use crate::modules::piece::{PieceType};
use crate::modules::zobrist::ZobristTable;
use super::bitboard::BitBoard;
use super::tables::*;

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum MoveType{
    Quiet =0,
    Capture =1,
    DoublePush =2,
    EnPassant =3,
    Promotion =4,
    PromotionCapture =5,
    KingSideCastling =6,
    QueenSideCastling =7,
}
const MOVE_TYPE_INDEX:[MoveType; 8] = 
[MoveType::Quiet, MoveType::Capture, MoveType::DoublePush, MoveType::EnPassant, MoveType::Promotion, MoveType::PromotionCapture, MoveType::KingSideCastling, MoveType::QueenSideCastling];

#[derive(Clone, Copy, PartialEq)]
pub struct Move(pub u32, pub i32);
//MoveInfo rayout
//(from:6)(to:6)(비어있음:6)(moving_p_type:3)(cap_p_type:3)(pro_p_type:3)(mov_type:3)
const FROM_SHIFT:i32 = 24;
const TO_SHIFT:i32 = 18;

const MOVING_SHIFT:i32 = 9;
const CAPTURE_SHIFT:i32 = 6;
const PROMOTION_SHIFT:i32 = 3;

fn parse_pos(pos:u8)->(char, char){
    let file = (pos % 8) + 'a' as u8;
    let rank = (pos / 8) + '1' as u8;
    (file as char, rank as char)
}

fn parse_type(t: PieceType) -> char {
    match t {
        PieceType::Bishop => 'B',
        PieceType::King => 'K',
        PieceType::Knight => 'N',
        PieceType::Pawn => 'P',
        PieceType::Queen => 'Q',
        PieceType::Rook => 'R',
        _=> unreachable!(),
    }
}

impl Move {
    /// UCI 좌표 표기 (예: e2e4, e7e8q, 캐슬링은 e1g1)
    pub fn to_uci(&self) -> String {
        let (f, t) = self.get_from_and_to_mask();
        let (from_file, from_rank) = parse_pos(f.trailing_zeros() as u8);
        let (to_file, to_rank) = parse_pos(t.trailing_zeros() as u8);
        let mut s = String::with_capacity(5);
        s.push(from_file);
        s.push(from_rank);
        s.push(to_file);
        s.push(to_rank);
        if matches!(self.get_move_type(), MoveType::Promotion | MoveType::PromotionCapture) {
            s.push(parse_type(self.get_promotion_piece_type()).to_ascii_lowercase());
        }
        s
    }

    pub fn to_string(&self) -> String {
        let (from_idx, to_idx) = {
            let (f, t) = self.get_from_and_to_mask();
            (f.trailing_zeros() as u8, t.trailing_zeros() as u8)
        };

        let (from_file, from_rank) = parse_pos(from_idx);
        let (to_file, to_rank) = parse_pos(to_idx);
        let piece_type = self.get_moving_piece_type();
        let move_type = self.get_move_type();

        match move_type {
            MoveType::KingSideCastling => "O-O".to_string(),
            MoveType::QueenSideCastling => "O-O-O".to_string(),
            _ => {
                let mut s = String::new();

                // 1. 기물 종류 (폰은 생략, 단 캡처 시 예외)
                if piece_type != PieceType::Pawn {
                    s.push(parse_type(piece_type));
                } else if matches!(move_type, MoveType::Capture | MoveType::EnPassant | MoveType::PromotionCapture) {
                    // 폰이 잡을 때는 출발 file 기록 (예: exd5)
                    s.push(from_file);
                }

                // 2. 캡처 표시
                if matches!(move_type, MoveType::Capture | MoveType::EnPassant | MoveType::PromotionCapture) {
                    s.push('x');
                }

                // 3. 도착지
                s.push(to_file);
                s.push(to_rank);

                // 4. 프로모션
                if matches!(move_type, MoveType::Promotion | MoveType::PromotionCapture) {
                    s.push('=');
                    s.push(parse_type(self.get_promotion_piece_type()));
                }

                s
            }
        }
    }
    
    #[inline(always)]
    pub fn print(&self){
        println!("이동한 기물: {:?}", self.get_moving_piece_type());
        println!("이동한 위치: {}", self.get_from_and_to_mask().0.trailing_zeros());
        println!("이동한 위치: {}", self.get_from_and_to_mask().1.trailing_zeros());
        println!("캡쳐한 기물: {:?}", self.get_captured_piece_type());
        println!("프로모션한 기물: {:?}", self.get_promotion_piece_type());
        println!("이동 타입: {:?}", self.get_move_type());
    }

    #[inline(always)]
    pub fn print_at_godot(&self){
        eprintln!("이동한 기물: {:?}", self.get_moving_piece_type());
        eprintln!("이동한 위치: {}", self.get_from_and_to_mask().0.trailing_zeros());
        eprintln!("이동한 위치: {}", self.get_from_and_to_mask().1.trailing_zeros());
        eprintln!("캡쳐한 기물: {:?}", self.get_captured_piece_type());
        eprintln!("프로모션한 기물: {:?}", self.get_promotion_piece_type());
        eprintln!("이동 타입: {:?}", self.get_move_type());
    }

    #[inline(always)]
    pub fn insert_pro_p_type(&mut self, p_type:PieceType){
        let mask = 0b111 << PROMOTION_SHIFT;
        self.0 &= !mask;
        self.0 |= (p_type as u32) << PROMOTION_SHIFT;
    }

    #[inline(always)]
    pub fn get_from_and_to_mask(&self) -> (u64, u64) {
        let from = (self.0 >> FROM_SHIFT) as u8;
        let to = ((self.0 >> TO_SHIFT) & 0b111111) as u8;
        (1u64 <<from, 1u64 <<to)
    }

    #[inline(always)]
    pub fn get_move_mask(&self) -> u64{
        let from = 1u64 << (self.0 >> FROM_SHIFT);
        let to = 1u64 << ((self.0 >> TO_SHIFT) & 0b111111);

        from|to
    }

    #[inline(always)]
    pub fn get_moving_piece_type(&self) -> PieceType {
        let idx = ((self.0 >> MOVING_SHIFT) & 0b111) as usize;
        PieceType::u8_as_piece_type(idx as u8)
    }

    #[inline(always)]
    pub fn get_captured_piece_type(&self) -> PieceType {
        let idx = ((self.0 >> CAPTURE_SHIFT) & 0b111) as usize;
        PieceType::u8_as_piece_type(idx as u8)
    }

    #[inline(always)]
    pub fn get_promotion_piece_type(&self) -> PieceType {
        let idx = ((self.0 >> PROMOTION_SHIFT) & 0b111) as usize;
        PieceType::u8_as_piece_type(idx as u8)
    }

    #[inline(always)]
    pub fn get_move_type(&self) -> MoveType{
        let idx = (self.0 & 0b111) as usize;
        MOVE_TYPE_INDEX[idx]
    }
}

const RANK_8:u64 = 0xFF << 56;
//const RANK_7:u64 = 0xFF << 48;
const RANK_6:u64 = 0xFF << 40;
const RANK_5:u64 = 0xFF << 32;
const RANK_4:u64 = 0xFF << 24;
const RANK_3:u64 = 0xFF << 16;
//const RANK_2:u64 = 0xFF << 8;
const RANK_1:u64 = 0xFF;

const NOT_FILE_A:u64 = !0x01_01_01_01_01_01_01_01;
const NOT_FILE_H:u64 = !0x80_80_80_80_80_80_80_80;

const PROMOTION_PIECES:[PieceType;4] = [PieceType::Bishop,PieceType::Knight, PieceType::Rook, PieceType::Queen];

impl BitBoard {
    pub fn generate_all_legal_moves(&mut self, z: &ZobristTable) -> Vec<Move>{

        let mut psedo = vec![];
        self.generate_all_pseudo_legal_moves(&mut psedo);
        let mut legal = Vec::with_capacity(psedo.len());

        for mv in psedo{
            self.make_move(mv, z);
            if !self.is_check(self.state_get_is_white_controll()){
                legal.push(mv);
            }
            self.unmake_move();
        }

        legal
    }

    #[inline(always)]
    pub fn do_with_mask<F>(&self, mut mask:u64,mut func :F)
    where 
        F: FnMut(u8) -> ()
        {
            while mask != 0 {
                let u =mask.trailing_zeros() as u8;
                func(u);

                mask &= mask -1
            }
        }
    
    pub fn generate_all_pseudo_legal_moves(&self, list:&mut Vec<Move>){
        list.clear();

        self.generate_pawn_move(list);
        self.generate_knight_move(list);
        self.generate_bishop_move(list);
        self.generate_rook_move(list);
        self.generate_queen_move(list);
        self.generate_king_move(list);
    }
    #[inline(always)]
    fn generate_pawn_move(&self, list: &mut Vec<Move>) {
        let is_white = self.state_get_is_white_controll();
        
        if is_white {
            let pawn_mask = self.white[PieceType::Pawn as usize];
            
            // 1. PUSH (전진)
            let single_push_mask = (pawn_mask << 8) & !self.total_occupied;
            
            // 프로모션 전진
            self.do_with_mask(single_push_mask & RANK_8, |to| {
                let from = to - 8;
                for pro_type in PROMOTION_PIECES {
                    list.push(self.create_move(from, to, PieceType::Pawn, PieceType::None, pro_type, MoveType::Promotion, 10000));
                }
            });

            // 일반 전진
            self.do_with_mask(single_push_mask & !RANK_8, |to| {
                list.push(self.create_move(to - 8, to, PieceType::Pawn, PieceType::None, PieceType::None, MoveType::Quiet, 0));
            });

            // 더블 푸쉬 (RANK 3 확인이 single_push_mask 이후에 와야 함)
            let double_push_mask = ((single_push_mask & RANK_3) << 8) & !self.total_occupied;
            self.do_with_mask(double_push_mask, |to| {
                list.push(self.create_move(to - 16, to, PieceType::Pawn, PieceType::None, PieceType::None, MoveType::DoublePush, 0));
            });

            // 2. CAPTURE (공격)
            let left_attack_mask = (pawn_mask <<7) & NOT_FILE_H & self.black_occupied;
            let promotion_left_attack_mask = left_attack_mask & RANK_8;
            let normal_left_attack = left_attack_mask & !RANK_8;
            let right_attack_mask = (pawn_mask <<9) & NOT_FILE_A & self.black_occupied;
            let promotion_right_attack = right_attack_mask & RANK_8;
            let normal_right_attack = right_attack_mask & !RANK_8;

            self.do_with_mask(promotion_left_attack_mask, |pos| {
                let cap_p = self.type_board[pos as usize];
                let score = calculate_mvv_lva(PieceType::Pawn, cap_p) + 10000; // 프로모션 가중치
                for pro_type in PROMOTION_PIECES {
                    list.push(self.create_move(pos - 7, pos, PieceType::Pawn, cap_p, pro_type, MoveType::PromotionCapture, score));
                }
            });

            self.do_with_mask(normal_left_attack, |pos| {
                let cap_p = self.type_board[pos as usize];
                let score = calculate_mvv_lva(PieceType::Pawn, cap_p);
                list.push(self.create_move(pos - 7, pos, PieceType::Pawn, cap_p, PieceType::None, MoveType::Capture, score));
            });

            self.do_with_mask(promotion_right_attack, |pos| {
                let cap_p = self.type_board[pos as usize];
                let score = calculate_mvv_lva(PieceType::Pawn, cap_p) + 10000; // 프로모션 가중치
                for pro_type in PROMOTION_PIECES {
                    list.push(self.create_move(pos - 9, pos, PieceType::Pawn, cap_p, pro_type, MoveType::PromotionCapture, score));
                }
            });

            self.do_with_mask(normal_right_attack, |pos| {
                let cap_p = self.type_board[pos as usize];
                let score = calculate_mvv_lva(PieceType::Pawn, cap_p);
                list.push(self.create_move(pos - 9, pos, PieceType::Pawn, cap_p, PieceType::None, MoveType::Capture, score));
            });

            // 3. EN PASSANT (앙파상)
            if let Some(ep_pos) = self.state_get_ep_pos() {
                // ep_pos는 폰이 이동할 빈 칸이므로, 공격하는 폰의 위치를 찾아야 함
                let attackers = BLACK_PAWN_ATTACKS[ep_pos as usize] & (self.white[PieceType::Pawn as usize] & RANK_5);
                self.do_with_mask(attackers, |from| {
                    list.push(self.create_move(from, ep_pos as u8, PieceType::Pawn, PieceType::Pawn, PieceType::None, MoveType::EnPassant, 1000));
                });
            }
        } else {
            let pawn_mask = self.black[PieceType::Pawn as usize];
            
            // 1. PUSH (전진)
            let single_push_mask = (pawn_mask >> 8) & !self.total_occupied;
            
            // 프로모션 전진
            self.do_with_mask(single_push_mask & RANK_1, |to| {
                let from = to + 8;
                for pro_type in PROMOTION_PIECES {
                    list.push(self.create_move(from, to, PieceType::Pawn, PieceType::None, pro_type, MoveType::Promotion, 10000));
                }
            });

            // 일반 전진
            self.do_with_mask(single_push_mask & !RANK_1, |to| {
                list.push(self.create_move(to + 8, to, PieceType::Pawn, PieceType::None, PieceType::None, MoveType::Quiet, 0));
            });

            // 더블 푸쉬 (RANK 4 확인이 single_push_mask 이후에 와야 함)
            let double_push_mask = ((single_push_mask & RANK_6) >> 8) & !self.total_occupied;
            self.do_with_mask(double_push_mask, |to| {
                list.push(self.create_move(to + 16, to, PieceType::Pawn, PieceType::None, PieceType::None, MoveType::DoublePush, 0));
            });

            // 2. CAPTURE (공격)
            let left_attack_mask = (pawn_mask >> 9) & NOT_FILE_H & self.white_occupied;
            let promotion_left_attack_mask = left_attack_mask & RANK_1;
            let normal_left_attack = left_attack_mask & !RANK_1;
            let right_attack_mask = (pawn_mask >> 7) & NOT_FILE_A & self.white_occupied;
            let promotion_right_attack_mask = right_attack_mask & RANK_1;
            let normal_right_attack = right_attack_mask & !RANK_1;

            self.do_with_mask(promotion_left_attack_mask, |pos| {
                let cap_p = self.type_board[pos as usize];
                let score = calculate_mvv_lva(PieceType::Pawn, cap_p) + 10000; // 프로모션 가중치
                for pro_type in PROMOTION_PIECES {
                    list.push(self.create_move(pos + 9, pos, PieceType::Pawn, cap_p, pro_type, MoveType::PromotionCapture, score));
                }
            });

            self.do_with_mask(normal_left_attack, |pos| {
                let cap_p = self.type_board[pos as usize];
                let score = calculate_mvv_lva(PieceType::Pawn, cap_p);
                list.push(self.create_move(pos + 9, pos, PieceType::Pawn, cap_p, PieceType::None, MoveType::Capture, score));
            });

            self.do_with_mask(promotion_right_attack_mask, |pos| {
                let cap_p = self.type_board[pos as usize];
                let score = calculate_mvv_lva(PieceType::Pawn, cap_p) + 10000; // 프로모션 가중치
                for pro_type in PROMOTION_PIECES {
                    list.push(self.create_move(pos + 7, pos, PieceType::Pawn, cap_p, pro_type, MoveType::PromotionCapture, score));
                }
            });

            self.do_with_mask(normal_right_attack, |pos| {
                let cap_p = self.type_board[pos as usize];
                let score = calculate_mvv_lva(PieceType::Pawn, cap_p);
                list.push(self.create_move(pos + 7, pos, PieceType::Pawn, cap_p, PieceType::None, MoveType::Capture, score));
            });

            // 3. EN PASSANT (앙파상)
            if let Some(ep_pos) = self.state_get_ep_pos() {
                // ep_pos는 폰이 이동할 빈 칸이므로, 공격하는 폰의 위치를 찾아야 함
                let attackers = WHITE_PAWN_ATTACKS[ep_pos as usize] & (self.black[PieceType::Pawn as usize] & RANK_4);
                self.do_with_mask(attackers, |from| {
                    list.push(self.create_move(from, ep_pos as u8, PieceType::Pawn, PieceType::Pawn, PieceType::None, MoveType::EnPassant, 1000));
                });
            }
        }
    }

    // 헬퍼 메서드: Move 조립을 한곳에서 관리
    #[inline(always)]
    fn create_move(&self, from: u8, to: u8, mov_p: PieceType, cap_p: PieceType, pro_p: PieceType, mt: MoveType, score: i32) -> Move {
        let data = (from as u32) << FROM_SHIFT |
                   (to as u32) << TO_SHIFT |
                   (mov_p as u32) << MOVING_SHIFT |
                   (cap_p as u32) << CAPTURE_SHIFT |
                   (pro_p as u32) << PROMOTION_SHIFT |
                   (mt as u32);
        Move(data, score)
    }

    #[inline(always)]
    fn generate_knight_move(&self, list: &mut Vec<Move>){
        if self.state_get_is_white_controll() {//백
            let knight_pos_mask = self.white[PieceType::Knight as usize];
            self.do_with_mask(knight_pos_mask, |from|{
                let move_mask = KNIGHT_ATTACKS[from as usize] & !self.white_occupied;
                let normal_mask = move_mask & !self.black_occupied;
                let capture_mask = move_mask & self.black_occupied;
                self.do_with_mask(normal_mask, |to|{
                    list.push(self.create_move(from, to, PieceType::Knight, PieceType::None, PieceType::None, MoveType::Quiet, 0));
                });
                self.do_with_mask(capture_mask, |to|{
                    let capture_p_type = self.type_board[to as usize];
                    let score = calculate_mvv_lva(PieceType::Knight, capture_p_type);
                    list.push(self.create_move(from, to, PieceType::Knight, capture_p_type, PieceType::None, MoveType::Capture, score));
                });
            });
        }else{//흑
            let knight_pos_mask = self.black[PieceType::Knight as usize];
            self.do_with_mask(knight_pos_mask, |from|{
                let move_mask = KNIGHT_ATTACKS[from as usize] & !self.black_occupied;
                let normal_mask = move_mask & !self.white_occupied;
                let capture_mask = move_mask & self.white_occupied;
                self.do_with_mask(normal_mask, |to|{
                    list.push(self.create_move(from, to, PieceType::Knight, PieceType::None, PieceType::None, MoveType::Quiet, 0));
                });
                self.do_with_mask(capture_mask, |to|{
                    let capture_p_type = self.type_board[to as usize];
                    let score = calculate_mvv_lva(PieceType::Knight, capture_p_type);
                    list.push(self.create_move(from, to, PieceType::Knight, capture_p_type, PieceType::None, MoveType::Capture, score));
                });
            });
        }
    }
    #[inline(always)]
    fn generate_bishop_move(&self, list: &mut Vec<Move>){
        if self.state_get_is_white_controll() {//백
            self.do_with_mask(self.white[PieceType::Bishop as usize], |from|{
                let move_mask = bishop_attacks(from as usize, self.total_occupied) & !self.white_occupied;
                let normal_mask = move_mask & !self.black_occupied;
                let capture_mask = move_mask & self.black_occupied;
                self.do_with_mask(normal_mask, |to|{
                    list.push(self.create_move(from, to, PieceType::Bishop, PieceType::None, PieceType::None, MoveType::Quiet, 0));
                });
                self.do_with_mask(capture_mask, |to|{
                    let cap_p_type = self.type_board[to as usize];
                    let score = calculate_mvv_lva(PieceType::Bishop, cap_p_type);
                    list.push(self.create_move(from, to, PieceType::Bishop, cap_p_type, PieceType::None, MoveType::Capture, score));
                });
            });
        }else{//흑
            self.do_with_mask(self.black[PieceType::Bishop as usize], |from|{
                let move_mask = bishop_attacks(from as usize, self.total_occupied) & !self.black_occupied;
                let normal_mask = move_mask & !self.white_occupied;
                let capture_mask = move_mask & self.white_occupied;
                self.do_with_mask(normal_mask, |to|{
                    list.push(self.create_move(from, to, PieceType::Bishop, PieceType::None, PieceType::None, MoveType::Quiet, 0));
                });
                self.do_with_mask(capture_mask, |to|{
                    let cap_p_type = self.type_board[to as usize];
                    let score = calculate_mvv_lva(PieceType::Bishop, cap_p_type);
                    list.push(self.create_move(from, to, PieceType::Bishop, cap_p_type, PieceType::None, MoveType::Capture, score));
                });
            });
        }
    }
    #[inline(always)]
    fn generate_rook_move(&self, list: &mut Vec<Move>){
        if self.state_get_is_white_controll() {//백
            self.do_with_mask(self.white[PieceType::Rook as usize], |from|{
                let move_mask = rook_attacks(from as usize, self.total_occupied) & !self.white_occupied;
                let normal_mask = move_mask & !self.black_occupied;
                let capture_mask = move_mask & self.black_occupied;
                self.do_with_mask(normal_mask, |to|{{
                    list.push(self.create_move(from, to, PieceType::Rook, PieceType::None, PieceType::None, MoveType::Quiet, 0));
                }});
                self.do_with_mask(capture_mask, |to|{
                    let cap_p_type = self.type_board[to as usize];
                    let score = calculate_mvv_lva(PieceType::Rook, cap_p_type);
                    list.push(self.create_move(from, to, PieceType::Rook, cap_p_type, PieceType::None, MoveType::Capture, score));
                });
            });
        }else{//흑
            self.do_with_mask(self.black[PieceType::Rook as usize], |from|{
                let move_mask = rook_attacks(from as usize, self.total_occupied) & !self.black_occupied;
                let normal_mask = move_mask & !self.white_occupied;
                let capture_mask = move_mask & self.white_occupied;
                self.do_with_mask(normal_mask, |to|{{
                    list.push(self.create_move(from, to, PieceType::Rook, PieceType::None, PieceType::None, MoveType::Quiet, 0));
                }});
                self.do_with_mask(capture_mask, |to|{
                    let cap_p_type = self.type_board[to as usize];
                    let score = calculate_mvv_lva(PieceType::Rook, cap_p_type);
                    list.push(self.create_move(from, to, PieceType::Rook, cap_p_type, PieceType::None, MoveType::Capture, score));
                });
            });
        }
    }
    #[inline(always)]
    fn generate_queen_move(&self, list: &mut Vec<Move>){
        if self.state_get_is_white_controll() {//백
            self.do_with_mask(self.white[PieceType::Queen as usize], |from|{
                let move_mask = (bishop_attacks(from as usize, self.total_occupied) | rook_attacks(from as usize, self.total_occupied)) & !self.white_occupied;
                let normal_mask = move_mask & !self.black_occupied;
                let capture_mask = move_mask & self.black_occupied;
                self.do_with_mask(normal_mask, |to|{
                    list.push(self.create_move(from, to, PieceType::Queen, PieceType::None, PieceType::None, MoveType::Quiet, 0));
                });
                self.do_with_mask(capture_mask, |to|{
                    let cap_p_type = self.type_board[to as usize];
                    let score = calculate_mvv_lva(PieceType::Queen, cap_p_type);
                    list.push(self.create_move(from, to, PieceType::Queen, cap_p_type, PieceType::None, MoveType::Capture, score));
                });
            });
        }else{//흑
            self.do_with_mask(self.black[PieceType::Queen as usize], |from|{
                let move_mask = (bishop_attacks(from as usize, self.total_occupied) | rook_attacks(from as usize, self.total_occupied)) & !self.black_occupied;
                let normal_mask = move_mask & !self.white_occupied;
                let capture_mask = move_mask & self.white_occupied;
                self.do_with_mask(normal_mask, |to|{
                    list.push(self.create_move(from, to, PieceType::Queen, PieceType::None, PieceType::None, MoveType::Quiet, 0));
                });
                self.do_with_mask(capture_mask, |to|{
                    let cap_p_type = self.type_board[to as usize];
                    let score = calculate_mvv_lva(PieceType::Queen, cap_p_type);
                    list.push(self.create_move(from, to, PieceType::Queen, cap_p_type, PieceType::None, MoveType::Capture, score));
                });
            });
        }
    }
    #[inline(always)]
    fn generate_king_move(&self, list: &mut Vec<Move>){
        if self.state_get_is_white_controll() {//백
            self.do_with_mask(self.white[PieceType::King as usize], |from|{
                let move_mask = KING_ATTACKS[from as usize] & !self.white_occupied;
                let normal_mask = move_mask & !self.black_occupied;
                let capture_mask = move_mask & self.black_occupied;
                self.do_with_mask(normal_mask, |to|{
                    list.push(self.create_move(from, to, PieceType::King, PieceType::None, PieceType::None, MoveType::Quiet, 0));
                });
                self.do_with_mask(capture_mask, |to|{
                    let cap_p_type = self.type_board[to as usize];
                    let score = calculate_mvv_lva(PieceType::King, cap_p_type);
                    list.push(self.create_move(from, to, PieceType::King, cap_p_type, PieceType::None, MoveType::Capture, score));
                });
            });
            if self.state_get_castling_right_wk() 
                && !self.is_check(false) 
                && !self.has_attack_to_square(5, false)
                && !self.has_attack_to_square(6, false)
                && (self.total_occupied & 0x60) == 0 {  // f1, g1 비어있는지
                list.push(self.create_move(4, 6, PieceType::King, PieceType::None, PieceType::None, MoveType::KingSideCastling, 60_000));
            }
            if self.state_get_castling_right_wq() 
                && !self.is_check(false) 
                && !self.has_attack_to_square(3, false)
                && !self.has_attack_to_square(2, false)
                && (self.total_occupied & 0x0E) == 0 {  // b1, c1, d1 비어있는지
                list.push(self.create_move(4, 2, PieceType::King, PieceType::None, PieceType::None, MoveType::QueenSideCastling, 60_000));
            }
        }else{//흑
            self.do_with_mask(self.black[PieceType::King as usize], |from|{
                let move_mask = KING_ATTACKS[from as usize] & !self.black_occupied;
                let normal_mask = move_mask & !self.white_occupied;
                let cpature_mask = move_mask & self.white_occupied;
                self.do_with_mask(normal_mask, |to|{
                    list.push(self.create_move(from, to, PieceType::King, PieceType::None, PieceType::None, MoveType::Quiet, 0));
                });
                self.do_with_mask(cpature_mask, |to|{
                    let cap_p_type = self.type_board[to as usize];
                    let score = calculate_mvv_lva(PieceType::King, cap_p_type);
                    list.push(self.create_move(from, to, PieceType::King, cap_p_type, PieceType::None, MoveType::Capture, score));
                });
            });
            if self.state_get_castling_right_bk() 
                && !self.is_check(true) 
                && !self.has_attack_to_square(61, true)
                && !self.has_attack_to_square(62, true)
                && (self.total_occupied & (0x60u64 << 56)) == 0 {
                list.push(self.create_move(60, 62, PieceType::King, PieceType::None, PieceType::None, MoveType::KingSideCastling, 60_000));
            }
            if self.state_get_castling_right_bq() 
                && !self.is_check(true) 
                && !self.has_attack_to_square(59, true)
                && !self.has_attack_to_square(58, true)
                && (self.total_occupied & (0x0Eu64 << 56)) == 0 {
                list.push(self.create_move(60, 58, PieceType::King, PieceType::None, PieceType::None, MoveType::QueenSideCastling, 60_000));
            }
        }
    }
}

#[inline(always)]
fn calculate_mvv_lva(moving_piece:PieceType, capture_piece:PieceType) -> i32 {
    capture_piece.get_value()*100 - moving_piece.get_value() 
}