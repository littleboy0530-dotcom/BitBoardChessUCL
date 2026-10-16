mod modules;

use crate::modules::bitboard::BitBoard;
use crate::modules::move_::{Move, MoveType};
use crate::modules::piece::PieceType;
use crate::modules::search::{self, MAX_DEPTH};
use crate::modules::zobrist::{TranspositionTable, ZobristTable};
use std::io::{self, BufRead};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

const ENGINE_NAME: &str = "BitBoardChess UCI";
const ENGINE_AUTHOR: &str = "Joon";
const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
// `go`에 아무 제한도 없을 때 사용하는 깊이 (Godot 버전의 MAX_SEARCH_DEPTH와 동일)
const DEFAULT_DEPTH: i32 = 10;
// GUI와의 통신 지연을 고려한 여유 시간(ms)
const MOVE_OVERHEAD_MS: u64 = 30;
const SEARCH_STACK_SIZE: usize = 64 * 1024 * 1024;
const MAX_THREADS: usize = 256;
const MAX_MULTI_PV: usize = 256;

/// UCI `position` 명령으로 받은 국면 (FEN + 이후 수순)
#[derive(Clone)]
struct Position {
    fen: String,
    moves: Vec<String>,
}

impl Position {
    fn startpos() -> Self {
        Position { fen: START_FEN.to_string(), moves: Vec::new() }
    }

    /// FEN과 수순을 적용한 보드를 생성. 잘못된 입력이면 Err
    fn build(&self, z: &ZobristTable) -> Result<BitBoard, String> {
        let mut board = board_from_fen(&self.fen, z)?;
        for mv_str in &self.moves {
            let mv = parse_uci_move(&mut board, mv_str, z)
                .ok_or_else(|| format!("illegal move: {}", mv_str))?;
            board.make_move(mv, z);
        }
        Ok(board)
    }
}

fn piece_from_char(c: char) -> Option<PieceType> {
    match c.to_ascii_lowercase() {
        'p' => Some(PieceType::Pawn),
        'n' => Some(PieceType::Knight),
        'b' => Some(PieceType::Bishop),
        'r' => Some(PieceType::Rook),
        'q' => Some(PieceType::Queen),
        'k' => Some(PieceType::King),
        _ => None,
    }
}

fn parse_square(s: &str) -> Option<u8> {
    let b = s.as_bytes();
    if b.len() != 2 { return None; }
    let file = b[0].wrapping_sub(b'a');
    let rank = b[1].wrapping_sub(b'1');
    if file < 8 && rank < 8 { Some(rank * 8 + file) } else { None }
}

/// FEN 문자열로 BitBoard를 구성 (a1 = 0, h8 = 63)
fn board_from_fen(fen: &str, z: &ZobristTable) -> Result<BitBoard, String> {
    let parts: Vec<&str> = fen.split_whitespace().collect();
    if parts.len() < 4 {
        return Err(format!("invalid fen: {}", fen));
    }

    let mut board = BitBoard::new_from_empty();

    // 1. 기물 배치
    let mut rank: i32 = 7;
    let mut file: i32 = 0;
    for c in parts[0].chars() {
        match c {
            '/' => { rank -= 1; file = 0; }
            '1'..='8' => file += c as i32 - '0' as i32,
            _ => {
                let p = piece_from_char(c).ok_or_else(|| format!("invalid fen piece: {}", c))?;
                if !(0..8).contains(&rank) || !(0..8).contains(&file) {
                    return Err(format!("invalid fen board: {}", parts[0]));
                }
                let sq = (rank * 8 + file) as usize;
                let bit = 1u64 << sq;
                if c.is_ascii_uppercase() {
                    board.white[p as usize] |= bit;
                } else {
                    board.black[p as usize] |= bit;
                }
                board.type_board[sq] = p;
                file += 1;
            }
        }
    }
    board.white_occupied = board.white.iter().fold(0, |o, b| o | b);
    board.black_occupied = board.black.iter().fold(0, |o, b| o | b);
    board.total_occupied = board.white_occupied | board.black_occupied;

    // 2. 상태 (차례 / 캐슬링 / 앙파상 / 수 카운터)
    let mut state: u32 = 0;
    match parts[1] {
        "w" => {}
        "b" => state |= 1,
        s => return Err(format!("invalid fen side: {}", s)),
    }
    let mut castling: u32 = 0;
    for c in parts[2].chars() {
        castling |= match c {
            'K' => 0b1000,
            'Q' => 0b0100,
            'k' => 0b0010,
            'q' => 0b0001,
            _ => 0,
        };
    }
    state |= castling << 2;
    board.state = state;

    if parts[3] != "-" {
        let ep = parse_square(parts[3]).ok_or_else(|| format!("invalid fen ep: {}", parts[3]))?;
        board.state_set_ep_pos(ep);
    }
    let half_move: u8 = parts.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
    let full_move: u16 = parts.get(5).and_then(|s| s.parse().ok()).unwrap_or(1);
    board.state_set_half_move(half_move.min(127));
    board.state_set_full_move(full_move.max(1));

    board.init_hash(z);
    board.ready_to_eval();
    Ok(board)
}

/// "e2e4", "e7e8q" 형식의 수를 현재 국면의 합법수와 매칭
fn parse_uci_move(board: &mut BitBoard, s: &str, z: &ZobristTable) -> Option<Move> {
    if s.len() < 4 { return None; }
    let from = parse_square(&s[0..2])?;
    let to = parse_square(&s[2..4])?;
    let promo = s.chars().nth(4).and_then(piece_from_char);

    board.generate_all_legal_moves(z).into_iter().find(|mv| {
        let (f, t) = mv.get_from_and_to_mask();
        if f.trailing_zeros() as u8 != from || t.trailing_zeros() as u8 != to {
            return false;
        }
        match mv.get_move_type() {
            MoveType::Promotion | MoveType::PromotionCapture => {
                // 승격 기물 미지정 시 퀸으로 간주
                mv.get_promotion_piece_type() == promo.unwrap_or(PieceType::Queen)
            }
            _ => true,
        }
    })
}

/// UCI `go` 명령 파라미터
#[derive(Default)]
struct GoParams {
    depth: Option<i32>,
    movetime: Option<u64>,
    wtime: Option<u64>,
    btime: Option<u64>,
    winc: u64,
    binc: u64,
    movestogo: Option<u64>,
    infinite: bool,
    perft: Option<u32>,
}

fn parse_go(tokens: &[&str]) -> GoParams {
    let mut p = GoParams::default();
    let mut i = 0;
    while i < tokens.len() {
        let next = tokens.get(i + 1).copied();
        let num = || next.and_then(|v| v.parse::<i64>().ok()).map(|v| v.max(0) as u64);
        match tokens[i] {
            "depth" => { p.depth = num().map(|v| v as i32); i += 1; }
            "movetime" => { p.movetime = num(); i += 1; }
            "wtime" => { p.wtime = num(); i += 1; }
            "btime" => { p.btime = num(); i += 1; }
            "winc" => { p.winc = num().unwrap_or(0); i += 1; }
            "binc" => { p.binc = num().unwrap_or(0); i += 1; }
            "movestogo" => { p.movestogo = num(); i += 1; }
            "perft" => { p.perft = num().map(|v| v as u32); i += 1; }
            "infinite" => p.infinite = true,
            _ => {}
        }
        i += 1;
    }
    p
}

/// 남은 시간 기준 이번 수에 사용할 시간(ms) 계산. None이면 시간 제한 없음
fn time_budget(p: &GoParams, white_to_move: bool) -> Option<u64> {
    if p.infinite { return None; }
    if let Some(mt) = p.movetime {
        return Some(mt.saturating_sub(MOVE_OVERHEAD_MS).max(1));
    }
    let (time, inc) = if white_to_move { (p.wtime?, p.winc) } else { (p.btime?, p.binc) };
    let moves_to_go = p.movestogo.unwrap_or(30).max(1);
    let budget = time / moves_to_go + inc * 3 / 4;
    let hard_cap = (time / 2).max(1);
    Some(budget.min(hard_cap).saturating_sub(MOVE_OVERHEAD_MS).max(1))
}

fn perft(board: &mut BitBoard, depth: u32, z: &ZobristTable) -> u64 {
    if depth == 0 { return 1; }
    let moves = board.generate_all_legal_moves(z);
    if depth == 1 { return moves.len() as u64; }
    let mut nodes = 0;
    for mv in moves {
        board.make_move(mv, z);
        nodes += perft(board, depth - 1, z);
        board.unmake_move();
    }
    nodes
}

fn run_perft(mut board: BitBoard, depth: u32, z: &ZobristTable) {
    let start = Instant::now();
    let mut total = 0;
    for mv in board.generate_all_legal_moves(z) {
        board.make_move(mv, z);
        let n = if depth > 1 { perft(&mut board, depth - 1, z) } else { 1 };
        board.unmake_move();
        println!("{}: {}", mv.to_uci(), n);
        total += n;
    }
    println!();
    println!("Nodes searched: {} ({} ms)", total, start.elapsed().as_millis());
}

/// 탐색 설정
struct SearchJob {
    position: Position,
    max_depth: i32,
    time_ms: Option<u64>,
    infinite: bool,
    threads: usize,
    multi_pv: usize,
}

fn spawn_with_stack<F: FnOnce() + Send + 'static>(name: String, f: F) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name(name)
        .stack_size(SEARCH_STACK_SIZE)
        .spawn(f)
        .expect("failed to spawn search thread")
}

/// 메인 탐색 스레드: 보조 스레드(Lazy SMP)를 띄우고 반복 심화 탐색 후 bestmove 출력
fn search_thread(job: SearchJob, z: Arc<ZobristTable>) {
    search::search_begin(job.time_ms);
    let mut board = match job.position.build(&z) {
        Ok(b) => b,
        Err(e) => { println!("info string error: {}", e); println!("bestmove 0000"); return; }
    };
    let legal = board.generate_all_legal_moves(&z);
    if legal.is_empty() {
        println!("info depth 0 score {}", if board.is_check(!board.state_get_is_white_controll()) { "mate 0" } else { "cp 0" });
        println!("bestmove 0000");
        return;
    }

    // 보조 스레드: 같은 국면을 독립적으로 탐색하며 공유 TT를 채움
    let helpers: Vec<JoinHandle<()>> = (1..job.threads)
        .map(|id| {
            let position = job.position.clone();
            let z = Arc::clone(&z);
            spawn_with_stack(format!("helper-{}", id), move || {
                if let Ok(mut b) = position.build(&z) {
                    b.iterative_search(MAX_DEPTH as i32, 1, id, &z);
                }
            })
        })
        .collect();

    let lines = board.iterative_search(job.max_depth, job.multi_pv, 0, &z);

    // UCI 규약: infinite 탐색은 stop을 받기 전까지 bestmove를 보내면 안 됨
    while job.infinite && !search::search_stopped() {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    search::request_stop();
    for h in helpers { let _ = h.join(); }

    // 깊이 1도 끝내지 못한 경우 등에는 첫 번째 합법수로 대체
    let best = lines.first().and_then(|l| l.pv.first()).copied().unwrap_or(legal[0]);
    let ponder = lines.first().and_then(|l| l.pv.get(1)).map(|m| format!(" ponder {}", m.to_uci())).unwrap_or_default();
    println!("bestmove {}{}", best.to_uci(), ponder);
}

struct Engine {
    z: Arc<ZobristTable>,
    position: Position,
    search: Option<JoinHandle<()>>,
    threads: usize,
    multi_pv: usize,
}

impl Engine {
    fn new() -> Self {
        TranspositionTable::global();
        Engine {
            z: Arc::new(ZobristTable::init()),
            position: Position::startpos(),
            search: None,
            threads: 1,
            multi_pv: 1,
        }
    }

    /// `setoption name <이름> [value <값>]`
    fn handle_setoption(&mut self, tokens: &[&str]) {
        let name_idx = tokens.iter().position(|&t| t == "name");
        let value_idx = tokens.iter().position(|&t| t == "value");
        let Some(ni) = name_idx else { return };
        let name = tokens[ni + 1..value_idx.unwrap_or(tokens.len())].join(" ").to_ascii_lowercase();
        let value = value_idx.map(|vi| tokens[vi + 1..].join(" ")).unwrap_or_default();
        let parsed = value.trim().parse::<usize>().ok();

        match name.as_str() {
            "threads" => match parsed {
                Some(v) => self.threads = v.clamp(1, MAX_THREADS),
                None => println!("info string invalid Threads value: {}", value),
            },
            "multipv" => match parsed {
                Some(v) => self.multi_pv = v.clamp(1, MAX_MULTI_PV),
                None => println!("info string invalid MultiPV value: {}", value),
            },
            _ => println!("info string unknown option: {}", name),
        }
    }

    fn stop_search(&mut self) {
        if let Some(handle) = self.search.take() {
            search::request_stop();
            let _ = handle.join();
        }
    }

    fn handle_position(&mut self, tokens: &[&str]) {
        let moves_idx = tokens.iter().position(|&t| t == "moves");
        let head = &tokens[..moves_idx.unwrap_or(tokens.len())];
        let fen = match head.first() {
            Some(&"startpos") => START_FEN.to_string(),
            Some(&"fen") => head[1..].join(" "),
            _ => return,
        };
        let moves = moves_idx
            .map(|i| tokens[i + 1..].iter().map(|s| s.to_string()).collect())
            .unwrap_or_default();
        let candidate = Position { fen, moves };
        match candidate.build(&self.z) {
            Ok(_) => self.position = candidate,
            Err(e) => println!("info string error: {}", e),
        }
    }

    fn handle_go(&mut self, tokens: &[&str]) {
        self.stop_search();
        let params = parse_go(tokens);
        let board = match self.position.build(&self.z) {
            Ok(b) => b,
            Err(e) => { println!("info string error: {}", e); return; }
        };

        if let Some(depth) = params.perft {
            run_perft(board, depth, &self.z);
            return;
        }

        let time_ms = time_budget(&params, board.state_get_is_white_controll());
        let max_depth = match (params.depth, time_ms, params.infinite) {
            (Some(d), _, _) => d,
            (None, Some(_), _) | (None, None, true) => MAX_DEPTH as i32,
            (None, None, false) => DEFAULT_DEPTH,
        };

        let job = SearchJob {
            position: self.position.clone(),
            max_depth,
            time_ms,
            infinite: params.infinite,
            threads: self.threads,
            multi_pv: self.multi_pv,
        };
        let z = Arc::clone(&self.z);
        self.search = Some(spawn_with_stack("search".into(), move || search_thread(job, z)));
    }

    /// 한 줄의 명령 처리. quit이면 false 반환
    fn handle_line(&mut self, line: &str) -> bool {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let Some(&cmd) = tokens.first() else { return true };
        match cmd {
            "uci" => {
                println!("id name {}", ENGINE_NAME);
                println!("id author {}", ENGINE_AUTHOR);
                println!("option name Threads type spin default 1 min 1 max {}", MAX_THREADS);
                println!("option name MultiPV type spin default 1 min 1 max {}", MAX_MULTI_PV);
                println!("uciok");
            }
            "isready" => println!("readyok"),
            "setoption" => {
                self.stop_search();
                self.handle_setoption(&tokens[1..]);
            }
            "ucinewgame" => {
                self.stop_search();
                TranspositionTable::global().clear();
                self.position = Position::startpos();
            }
            "position" => {
                self.stop_search();
                self.handle_position(&tokens[1..]);
            }
            "go" => self.handle_go(&tokens[1..]),
            "stop" => self.stop_search(),
            "d" => match self.position.build(&self.z) {
                Ok(b) => {
                    b.display();
                    let threefold = b.history.is_repetition_draw(b.hash, b.state_get_half_move(), 0);
                    println!("Threefold repetition: {}", if threefold { "yes (draw)" } else { "no" });
                }
                Err(e) => println!("info string error: {}", e),
            },
            "quit" => {
                self.stop_search();
                return false;
            }
            _ => {}
        }
        true
    }
}

fn main() {
    let mut engine = Engine::new();
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if !engine.handle_line(line.trim()) {
            return;
        }
    }
    // stdin이 닫히면 진행 중인 탐색을 마무리하고 종료
    if let Some(handle) = engine.search.take() {
        let _ = handle.join();
    }
}
