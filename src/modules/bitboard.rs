
use crate::modules::zobrist::ZobristTable;

use super::piece::*;

const START_PIECE_BOARD:[PieceType;64] = 
    [PieceType::Rook, PieceType::Knight, PieceType::Bishop, PieceType::Queen, PieceType::King, PieceType::Bishop, PieceType::Knight, PieceType::Rook,
    PieceType::Pawn, PieceType::Pawn, PieceType::Pawn, PieceType::Pawn, PieceType::Pawn, PieceType::Pawn, PieceType::Pawn, PieceType::Pawn,
    PieceType::None, PieceType::None, PieceType::None, PieceType::None, PieceType::None, PieceType::None, PieceType::None, PieceType::None,
    PieceType::None, PieceType::None, PieceType::None, PieceType::None, PieceType::None, PieceType::None, PieceType::None, PieceType::None,
    PieceType::None, PieceType::None, PieceType::None, PieceType::None, PieceType::None, PieceType::None, PieceType::None, PieceType::None,
    PieceType::None, PieceType::None, PieceType::None, PieceType::None, PieceType::None, PieceType::None, PieceType::None, PieceType::None,
    PieceType::Pawn, PieceType::Pawn, PieceType::Pawn, PieceType::Pawn, PieceType::Pawn, PieceType::Pawn, PieceType::Pawn, PieceType::Pawn,
    PieceType::Rook, PieceType::Knight, PieceType::Bishop, PieceType::Queen, PieceType::King, PieceType::Bishop, PieceType::Knight, PieceType::Rook,];

pub struct BitBoard {
    pub history:BoardHistory,
    pub type_board:[PieceType;64],
    pub white:[u64;6],
    pub black:[u64;6],
    pub white_occupied:u64,
    pub black_occupied:u64,
    pub total_occupied:u64,
    pub mg:[i32;2],
    pub eg:[i32;2],
    pub hash: u64,
    pub state:u32,
    pub game_phase:i32,
}
//state layout
//(full move)(half move)(ep_pos)(castling flag)(ep_flag)(controll)
//     13         7         6          4           1        1
const FULL_MOVE_SHIFT:u32 = 19;
const HALF_MOVE_SHIFT:u32 = 12;
const EP_POS_SHIFT:u32 = 6;
const CASTLING_FLAG_SHIFT:u32 = 2;
const EP_FLAG_SHIFT:u32 = 1;
const POS_MASK:u32 = 0b111111;

impl BitBoard {
    pub fn display(&self) {
        // 체스판은 보통 8행(Rank 8)부터 시작하므로 역순으로 반복합니다.
        for rank in (0..8).rev() {
            print!("{}", rank + 1); // 행 번호 출력
            for file in 0..8 {
                let square = rank * 8 + file;
                let piece_type = self.type_board[square];
                
                // 기물이 있는지 확인 (occupied 비트보드 활용)
                let is_white = (self.white_occupied >> square) & 1 == 1;
                let is_black = (self.black_occupied >> square) & 1 == 1;
                
                if is_white {
                    print!(" {}", BitBoard::get_piece_char(piece_type, true));
                } else if is_black {
                    print!(" {}", BitBoard::get_piece_char(piece_type, false));
                } else {
                    print!(" ."); // 빈 칸
                }
            }
            println!()
        }
        println!(" a b c d e f g h");
    }

    #[inline(always)]
    fn get_piece_char(piece_type: PieceType, is_white: bool) -> char {
        let c = match piece_type {
            PieceType::Pawn => 'P',
            PieceType::Knight => 'N',
            PieceType::Bishop => 'B',
            PieceType::Rook => 'R',
            PieceType::Queen => 'Q',
            PieceType::King => 'K',
            _ => '.', // 기물이 없는 경우 (사실 위 로직에서 필터링됨)
        };
        
        if is_white { c } else { c.to_ascii_lowercase() }
    }

    #[inline(always)]
    pub fn print_state(&self){
        eprintln!("ep_pos:{} castling_flag: {:b} white_controll: {}", self.state_get_ep_pos().unwrap_or(100),(self.state >> 2) & 0b1111, self.state_get_is_white_controll());
    }
    #[inline(always)]
    pub fn new_from_empty() -> Self {
        Self{history: BoardHistory::new_from_empty(),
        type_board: [PieceType::None; 64],
        white:[0u64;6], black:[0u64;6], 
        white_occupied: 0u64, black_occupied: 0u64,
        total_occupied: 0u64, mg:[0;2],eg:[0;2],
        hash: 0u64, state: 0u32, game_phase: 0}
    }

    pub fn new_from_start_position() -> Self {
        let white = [0xFF<<8, 0b01000010, 0b00100100, 0b10000001, 0b00001000, 0b00010000];
                    //            Pawn    Knight      Bishop      Rook        Queen(d1)   King(e1)

        let black = [0xFF<<48, 0b01000010<<56, 0b00100100<<56, 0b10000001<<56, 0b00001000<<56, 0b00010000<<56];
        let white_occupied = white.iter().fold(0u64, |occ, &w| occ | w);
        let black_occupied = black.iter().fold(0u64, |occ, b| occ | b);
        let total_occupied = white_occupied | black_occupied;

        BitBoard{history: BoardHistory::new_from_empty(), 
                type_board: START_PIECE_BOARD,
                white, black, white_occupied, black_occupied, total_occupied,
                mg:[0;2],eg:[0;2],hash: 0u64,
                state: BitBoard::state_new_from_start(), game_phase: 0}
    }

    #[inline(always)]
    pub fn init_hash(&mut self, z: &ZobristTable){
        let mut h =0;

        for (t, w) in self.white.iter().enumerate(){
            self.do_with_mask(*w, |pos|{
                h ^= z.pieces[t<<1][pos as usize];
            });
        }
        for (t, b) in self.black.iter().enumerate(){
            self.do_with_mask(*b, |pos|{
                h ^= z.pieces[(t<<1)|1][pos as usize];
            });
        }

        h ^= z.castle[self.state_get_castling_flag() as usize];
        if let Some(ep) = self.state_get_ep_pos(){
            h ^= z.en_passant[(ep & 0b111)as usize];
        }
        if !self.state_get_is_white_controll(){
            h ^= z.side;
        }

        self.hash = h;
    }

    #[inline(always)]
    pub fn state_new_from_start() -> u32{
        let result = 1<<FULL_MOVE_SHIFT 
        | 0b1111<<CASTLING_FLAG_SHIFT;

        result
    }

    #[inline(always)]
    pub fn state_lose_castling_right_by_bit(&mut self, bit:u8){
        self.state &= !((bit as u32) << CASTLING_FLAG_SHIFT);
    }

    #[inline(always)]
    pub fn state_set_ep_pos(&mut self, pos:u8){
        self.state |= (pos as u32) << EP_POS_SHIFT | 1u32 <<EP_FLAG_SHIFT;
    }

    #[inline(always)]
    pub fn state_ep_pos_clear(&mut self){
        let ep_pos_mask = 0b111111 <<EP_POS_SHIFT;
        let ep_flag_mask = 0b10;
        self.state &= !(ep_flag_mask | ep_pos_mask);
    }

    #[inline(always)]
    pub fn state_set_controll_change(&mut self) {
        self.state ^= 1;
    }
    
    #[inline(always)]
    pub fn state_get_castling_right_wk(&self) -> bool {
        (self.state >> CASTLING_FLAG_SHIFT) & 0b1000 != 0
    }
    #[inline(always)]
    pub fn state_get_castling_right_wq(&self) -> bool {
        (self.state >> CASTLING_FLAG_SHIFT) & 0b0100 != 0
    }
    #[inline(always)]
    pub fn state_get_castling_right_bk(&self) -> bool {
        (self.state >> CASTLING_FLAG_SHIFT) & 0b0010 != 0
    }
    #[inline(always)]
    pub fn state_get_castling_right_bq(&self) -> bool {
        (self.state >> CASTLING_FLAG_SHIFT) & 0b0001 != 0
    }

    #[inline(always)]
    pub fn state_get_is_white_controll(&self) -> bool {
        self.state & 1 != 1 
    }

    #[inline(always)]
    pub fn state_get_castling_flag(&self) -> u64 {
        ((self.state >> CASTLING_FLAG_SHIFT) & 0b1111) as u64
    }

    #[inline(always)]
    pub fn state_get_ep_pos(&self) -> Option<u8>{
        if (self.state & 0b10) == 0 {
            None
        }else{
            Some(((self.state >> EP_POS_SHIFT) & POS_MASK) as u8)
        }
    }

    #[inline(always)]
    pub fn state_get_half_move(&self) -> u8 {
        ((self.state >> HALF_MOVE_SHIFT) & 0b1111111) as u8
    }

    #[inline(always)]
    pub fn state_set_half_move(&mut self, half_move: u8) {
        let mask = 0b1111111u32 << HALF_MOVE_SHIFT;
        self.state = (self.state & !mask) | (((half_move as u32) & 0b1111111) << HALF_MOVE_SHIFT);
    }

    #[inline(always)]
    pub fn state_get_full_move(&self) -> u16 {
        ((self.state >> FULL_MOVE_SHIFT) & 0x1FFF) as u16
    }

    #[inline(always)]
    pub fn state_set_full_move(&mut self, full_move: u16) {
        let mask = 0x1FFFu32 << FULL_MOVE_SHIFT;
        self.state = (self.state & !mask) | (((full_move as u32) & 0x1FFF) << FULL_MOVE_SHIFT);
    }

    #[inline(always)]
    pub fn state_increment_half_move(&mut self) {
        let current = self.state_get_half_move();
        if current < 127{
            self.state_set_half_move(current +1);
        }
        
    }

    #[inline(always)]
    pub fn state_reset_half_move(&mut self) {
        self.state_set_half_move(0);
    }

    #[inline(always)]
    pub fn state_increment_full_move(&mut self) {
        let current = self.state_get_full_move();
        self.state_set_full_move(current.saturating_add(1));
    }
}

pub struct BoardHistory {
    moves:[u32; 1024],//<-move_info
    states:[u32; 1024],
    hash:[u64; 1024],
    index:usize,
}

impl BoardHistory {
    #[inline(always)]
    fn new_from_empty() -> Self{
        BoardHistory{moves:[0u32;1024], states:[0u32;1024], hash:[0u64;1024], index: 0}
    }

    ///move_info, state튜플을 인자로 받아서 저장함
    #[inline(always)]
    pub fn push_to_tuple(&mut self, tuple:(u32,u32, u64)) {
        self.index +=1;
        self.moves[self.index] = tuple.0;
        self.states[self.index] = tuple.1;
        self.hash[self.index] = tuple.2;
    }

    //마지막에 저장된 move_info와 state를 반환함
    #[inline(always)]
    pub fn pop_to_tuple(&mut self) -> (u32, u32, u64){
        self.index -=1;
        (self.moves[self.index +1], self.states[self.index +1], self.hash[self.index +1])
    }

    #[inline(always)]
    pub fn peek_to_tuple(&self) -> (u32, u32, u64){
        (self.moves[self.index], self.states[self.index], self.hash[self.index])
    }

    #[inline(always)]
    pub fn is_repetition(&self, hash: u64, half_move: u8) -> bool {
        // self.hash[self.index + 1 - p] = p 수(ply) 전의 포지션 해시
        let limit = (half_move as usize).min(self.index);
        let mut p = 2;
        while p <= limit {
            if self.hash[self.index + 1 - p] == hash {
                return true;
            }
            p += 2;
        }
        false
    }

    /// 반복에 의한 무승부 판정
    /// - `ply`: 탐색 루트로부터의 거리. 탐색 트리 안(루트 포함)에서 반복되면 바로 무승부로 본다
    /// - 루트 이전(실제 대국 기록)의 국면과만 겹치면 3회 동형(이전 2회 등장)일 때 무승부
    #[inline(always)]
    pub fn is_repetition_draw(&self, hash: u64, half_move: u8, ply: usize) -> bool {
        let limit = (half_move as usize).min(self.index);
        let mut count = 0;
        let mut p = 4; // 같은 국면이 되려면 최소 4ply가 필요
        while p <= limit {
            if self.hash[self.index + 1 - p] == hash {
                if p <= ply {
                    return true;
                }
                count += 1;
                if count >= 2 {
                    return true;
                }
            }
            p += 2;
        }
        false
    }
}