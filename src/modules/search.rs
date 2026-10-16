use crate::modules::zobrist::*;
use crate::modules::{bitboard::*, piece::*, move_::*};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

const INF: i32 = 1000_000;
const MATE_SCORE: i32 = 900_000;
pub const MAX_DEPTH: usize = 64;

// --- UCI 탐색 제어 (stop 명령 / 시간 제한) ---
static STOP: AtomicBool = AtomicBool::new(false);
// 프로세스 기준 시각으로부터의 마감 시간(ms). 0이면 시간 제한 없음
static DEADLINE_MS: AtomicU64 = AtomicU64::new(0);
// 탐색 시작 시각(ms)과 모든 스레드의 누적 노드 수
static START_MS: AtomicU64 = AtomicU64::new(0);
static NODES: AtomicU64 = AtomicU64::new(0);

fn elapsed_ms() -> u64 {
    (clock_origin().elapsed().as_millis() as u64).saturating_sub(START_MS.load(Ordering::Relaxed))
}

fn clock_origin() -> &'static Instant {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    ORIGIN.get_or_init(Instant::now)
}

/// 탐색 시작 전에 호출: 정지 플래그 초기화 및 시간 제한(ms) 설정
pub fn search_begin(time_limit_ms: Option<u64>) {
    STOP.store(false, Ordering::SeqCst);
    NODES.store(0, Ordering::SeqCst);
    let now = clock_origin().elapsed().as_millis() as u64;
    START_MS.store(now, Ordering::SeqCst);
    let deadline = match time_limit_ms {
        Some(ms) => now + ms.max(1),
        None => 0,
    };
    DEADLINE_MS.store(deadline, Ordering::SeqCst);
}

/// UCI `stop` 명령 등으로 탐색 중단 요청
pub fn request_stop() {
    STOP.store(true, Ordering::SeqCst);
}

#[inline(always)]
pub fn search_stopped() -> bool {
    STOP.load(Ordering::Relaxed)
}

#[inline(always)]
fn check_time(node_counter: u64) {
    if node_counter & 2047 == 0 {
        let deadline = DEADLINE_MS.load(Ordering::Relaxed);
        if deadline != 0 && clock_origin().elapsed().as_millis() as u64 >= deadline {
            STOP.store(true, Ordering::Relaxed);
        }
    }
}

/// 탐색 점수를 UCI score 문자열로 변환 (cp / mate)
fn uci_score(score: i32) -> String {
    if score >= MATE_SCORE - 100 {
        format!("mate {}", (MATE_SCORE - score + 1) / 2)
    } else if score <= -MATE_SCORE + 100 {
        format!("mate -{}", (MATE_SCORE + score) / 2)
    } else {
        format!("cp {}", score)
    }
}

impl BitBoard {
    /// 메인 네가맥스 탐색 (알파-베타 가지치기 포함)
    pub fn negamax(
        &mut self,
        depth: i32,
        mut alpha: i32,
        beta: i32,
        ply: usize,
        z: &ZobristTable,
        killers: &mut [[Option<u32>; 2]; MAX_DEPTH],
        node_counter: &mut u64,
    ) -> i32 {
        *node_counter += 1;
        check_time(*node_counter);
        if search_stopped() { return 0; }

        // 0. 반복 무승부 (3회 동형)
        if ply > 0 && self.history.is_repetition_draw(self.hash, self.state_get_half_move(), ply) {
            return 0;
        }

        // 1. 정적 탐색 (지평선 효과 방지)
        if depth <= 0 {
            return self.quiescence(alpha, beta, node_counter, z);
        }

        let hash = self.hash;
        let tt = TranspositionTable::global();
        let entry = tt.get_entry(hash);
        let mut tt_move = None;

        // 2. 전치 테이블(TT) 조회
        if entry.hash_key == hash {
            // if (self.state_get_is_white_controll() && entry.repeat.0 == 2) 
            // && (!self.state_get_is_white_controll() && entry.repeat.1 ==2){
            //     return 0
            // }//반복 무승부
            
            tt_move = Some(entry.best_move);
            if entry.depth >= (depth as u8) {
                let mut score = entry.score;
                // 메이트 스코어 보정
                if score > MATE_SCORE { score -= ply as i32; }
                else if score < -MATE_SCORE { score += ply as i32; }

                match entry.flag {
                    TtFlag::Exact => return score,
                    TtFlag::LowerBound => { if score >= beta { return beta; } },
                    TtFlag::UpperBound => { if score <= alpha { return alpha; } },
                }
            }
        }

        // 2.5 Null Move Pruning
        let in_check = self.is_check(!self.state_get_is_white_controll());
        if ply > 0
            && depth >= 3
            && !in_check
            && beta.abs() < MATE_SCORE - 100
            && self.history.peek_to_tuple().0 != 0   // 직전 수가 null move면 연속 금지
            && self.has_non_pawn_material()          // 폰 엔드게임(추크추방) 방지
            && self.eval() >= beta
        {
            let r = if depth > 6 { 3 } else { 2 };
            self.make_null_move(z);
            let score = -self.negamax(depth - 1 - r, -beta, -beta + 1, ply + 1, z, killers, node_counter);
            self.unmake_null_move();
            if search_stopped() { return 0; }
            if score >= beta {
                return beta;
            }
        }

        // 3. 모든 가능한 수 생성
        let mut moves = Vec::with_capacity(64);
        self.generate_all_pseudo_legal_moves(&mut moves);

        if moves.is_empty() {
            // 현재 체크 상태면 체크메이트, 아니면 스테일메이트
            return if self.is_check(self.state_get_is_white_controll()) {
                -MATE_SCORE + ply as i32 
            } else {
                0 
            };
        }

        // 4. 이동 정렬 가중치 부여 (내장 함수용)
        for m in moves.iter_mut() {
            let m_type = m.0 & 0b111;
            let is_capture = 
            m_type == MoveType::EnPassant as u32 ||
            m_type == MoveType::Capture  as u32 ||
            m_type == MoveType::PromotionCapture as u32;

            if Some(m.0) == tt_move {
                m.1 = 2000_000;//TT수를 최우선으로 
            }else if is_capture {
                m.1 += 1000_000;//capture를 다음으로
            }else if killers[ply][0] == Some(m.0) {
                m.1 = 90_000;
            }else if killers[ply][0] == Some(m.0) {
                m.1 = 80_000;
            }else if m_type == MoveType::KingSideCastling as u32 ||
                     m_type == MoveType::QueenSideCastling as u32 {
                m.1 = 0;
            }
        }

        // Rust 내장 정렬: 점수(m.1)의 내림차순 정렬
        moves.sort_unstable_by_key(|m| -m.1);

        let mut legal_move_count = 0;
        let mut best_move_in_node = 0;
        let mut best_score = -INF;
        let old_alpha = alpha;
        let opponent_color_is_white = !self.state_get_is_white_controll();

        // 5. 탐색 루프
        for mv in moves {
            self.make_move(mv, z);
            if self.is_check(opponent_color_is_white){
                self.unmake_move();
                continue;
            }
            legal_move_count +=1;
            let score = -self.negamax(depth - 1, -beta, -alpha, ply + 1, z, killers, node_counter);
            self.unmake_move(); // simulation.rs 정의에 맞춰 인자 제거
            // 중단된 탐색 결과는 TT/킬러에 반영하지 않음
            if search_stopped() { return 0; }

            if score >= beta {
                // 베타 컷오프 발생: 킬러 무브 기록 (캡처가 아닌 일반 수일 때)
                if (mv.0 & 0b111) == (MoveType::Quiet as u32) {
                    killers[ply][1] = killers[ply][0];
                    killers[ply][0] = Some(mv.0);
                }
                
                // TT 업데이트 (LowerBound)
                tt.store_entry(hash, TtEntry {
                    hash_key: hash,
                    best_move: mv.0,
                    score: beta,
                    depth: depth as u8,
                    flag: TtFlag::LowerBound,
                    //repeat: entry.repeat,
                });
                return beta;
            }

            if score > best_score {
                best_score = score;
                best_move_in_node = mv.0;
                if score > alpha {
                    alpha = score;
                }
            }
        }

        // 모든 수를 다 돌았는데 legal_move_count가 0이면 체크메이트 또는 스테일메이트
        if legal_move_count == 0 {
            return if self.is_check(opponent_color_is_white) {
                -MATE_SCORE + ply as i32 
            } else { 0 };
        }

        // 6. 탐색 결과 TT 저장
        let flag = if best_score <= old_alpha {
            TtFlag::UpperBound
        } else {
            TtFlag::Exact
        };

        tt.store_entry(hash, TtEntry {
            hash_key: hash,
            best_move: best_move_in_node,
            score: best_score,
            depth: depth as u8,
            flag,
            //repeat: entry.repeat,
        });

        alpha
    }

    /// 정적 탐색: 기물 캡처 상황이 정리될 때까지 추가 탐색
    pub fn quiescence(&mut self, mut alpha: i32, beta: i32, node_counter: &mut u64, z: &ZobristTable) -> i32 {
        *node_counter += 1;
        check_time(*node_counter);
        if search_stopped() { return 0; }

        let stand_pat = self.eval();
        if stand_pat >= beta { return beta; }
        if alpha < stand_pat { alpha = stand_pat; }

        let mut moves: Vec<Move> = Vec::with_capacity(64);
        self.generate_all_pseudo_legal_moves(&mut moves);
        // 캡처와 관련된 이동만 남김 (Capture=1, EnPassant=3, PromotionCapture=5)
        moves.retain(|m| {
            let m_type = m.0 & 0b111;
            m_type == 1 || m_type == 3 || m_type == 5
        });
        
        // MVV-LVA 순으로 정렬
        moves.sort_unstable_by_key(|m| -m.1);

        let opponent_color_is_white = !self.state_get_is_white_controll();

        for mv in moves {
            self.make_move(mv, z);

            if self.is_check(opponent_color_is_white) {
                self.unmake_move();
                continue;
            }

            let score = -self.quiescence(-beta, -alpha, node_counter, z);
            self.unmake_move();

            if score >= beta { return beta; }
            if score > alpha { alpha = score; }
        }

        alpha
    }

    /// 루트 탐색: `excluded`에 있는 수를 제외한 루트 수 중 최선의 수와 점수
    /// MultiPV는 이미 찾은 수를 제외하고 반복 호출하여 구현한다
    fn search_root(
        &mut self,
        depth: i32,
        root_moves: &[Move],
        excluded: &[u32],
        z: &ZobristTable,
        killers: &mut [[Option<u32>; 2]; MAX_DEPTH],
        node_counter: &mut u64,
    ) -> Option<(Move, i32)> {
        let mut alpha = -INF;
        let beta = INF;
        let mut best: Option<(Move, i32)> = None;

        for &mv in root_moves {
            if excluded.contains(&mv.0) { continue; }
            self.make_move(mv, z);
            let score = -self.negamax(depth - 1, -beta, -alpha, 1, z, killers, node_counter);
            self.unmake_move();
            if search_stopped() { return None; }

            if best.map_or(true, |(_, s)| score > s) {
                best = Some((mv, score));
                if score > alpha { alpha = score; }
            }
        }

        // 첫 번째 PV만 루트 TT에 기록 (다음 깊이의 정렬과 PV 추출에 사용)
        if excluded.is_empty() {
            if let Some((mv, score)) = best {
                TranspositionTable::global().store_entry(self.hash, TtEntry {
                    hash_key: self.hash,
                    best_move: mv.0,
                    score,
                    depth: depth as u8,
                    flag: TtFlag::Exact,
                });
            }
        }
        best
    }

    /// TT를 따라가며 PV(주요 변화)를 추출
    fn extract_pv(&mut self, first: Move, max_len: usize, z: &ZobristTable) -> Vec<Move> {
        let mut pv = vec![first];
        self.make_move(first, z);
        let mut made = 1;
        let tt = TranspositionTable::global();

        while pv.len() < max_len {
            if self.history.is_repetition_draw(self.hash, self.state_get_half_move(), made) { break; }
            let entry = tt.get_entry(self.hash);
            if entry.hash_key != self.hash || entry.best_move == 0 { break; }
            // TT 충돌로 잘못된 수가 섞이지 않도록 합법수인지 확인
            let Some(mv) = self.generate_all_legal_moves(z).into_iter().find(|m| m.0 == entry.best_move) else { break };
            pv.push(mv);
            self.make_move(mv, z);
            made += 1;
        }

        for _ in 0..made { self.unmake_move(); }
        pv
    }

    /// 반복적 심화 탐색 (Iterative Deepening) + MultiPV
    /// - `thread_id` 0(메인 스레드)만 info를 출력하고, 나머지는 TT를 채우는 보조 스레드(Lazy SMP)
    /// - 마지막으로 완료된 깊이의 PV 목록(점수 내림차순)을 반환
    pub fn iterative_search(
        &mut self,
        max_target_depth: i32,
        multi_pv: usize,
        thread_id: usize,
        z: &ZobristTable,
    ) -> Vec<PvLine> {
        let mut root_moves = self.generate_all_legal_moves(z);
        if root_moves.is_empty() { return Vec::new(); }
        root_moves.sort_by_key(|m| -m.1);
        // 보조 스레드는 루트 수 순서를 바꿔서 서로 다른 가지를 먼저 탐색하도록 함
        if thread_id > 0 {
            let len = root_moves.len();
            root_moves.rotate_left(thread_id % len);
        }

        let multi_pv = multi_pv.clamp(1, root_moves.len());
        let max_target_depth = max_target_depth.clamp(1, MAX_DEPTH as i32 - 4);
        // 보조 스레드는 홀수 번째마다 한 단계 깊게 시작해 탐색 깊이를 분산
        let start_depth = if thread_id % 2 == 1 { 2.min(max_target_depth) } else { 1 };

        let mut killers = [[None; 2]; MAX_DEPTH];
        let mut completed: Vec<PvLine> = Vec::new();

        for depth in start_depth..=max_target_depth {
            let mut node_count = 0;
            let mut lines: Vec<(Move, i32)> = Vec::with_capacity(multi_pv);
            let mut excluded: Vec<u32> = Vec::with_capacity(multi_pv);

            for _ in 0..multi_pv {
                match self.search_root(depth, &root_moves, &excluded, z, &mut killers, &mut node_count) {
                    Some((mv, score)) => {
                        excluded.push(mv.0);
                        lines.push((mv, score));
                    }
                    None => break,
                }
            }
            NODES.fetch_add(node_count, Ordering::Relaxed);

            // 시간 초과/stop으로 중단된 깊이의 결과는 버림
            if search_stopped() || lines.len() < multi_pv { break; }

            lines.sort_by_key(|&(_, s)| -s);
            // 다음 깊이에서는 이번 PV 수들을 먼저 탐색
            for (i, (mv, _)) in lines.iter().enumerate() {
                if let Some(pos) = root_moves.iter().position(|m| m.0 == mv.0) {
                    let m = root_moves.remove(pos);
                    root_moves.insert(i, m);
                }
            }

            completed = lines.iter()
                .map(|&(mv, score)| PvLine { pv: self.extract_pv(mv, depth as usize, z), score })
                .collect();

            if thread_id == 0 {
                let elapsed = elapsed_ms();
                let nodes = NODES.load(Ordering::Relaxed);
                let nps = if elapsed > 0 { nodes * 1000 / elapsed } else { 0 };
                for (i, line) in completed.iter().enumerate() {
                    let pv: Vec<String> = line.pv.iter().map(|m| m.to_uci()).collect();
                    println!("info depth {} multipv {} score {} nodes {} nps {} time {} pv {}",
                             depth, i + 1, uci_score(line.score), nodes, nps, elapsed, pv.join(" "));
                }
            }

            // 메이트 상황이면 더 이상 깊게 탐색하지 않음
            if completed[0].score.abs() >= MATE_SCORE - 100 { break; }
        }

        completed
    }
}

/// MultiPV 한 줄: 수순과 점수
pub struct PvLine {
    pub pv: Vec<Move>,
    pub score: i32,
}
