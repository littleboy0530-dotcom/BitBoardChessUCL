use crate::modules::{bitboard::*, eval::{EG_TABLE, MG_TABLE}, move_::{self, Move, MoveType}, piece::PieceType, tables::{BLACK_PAWN_ATTACKS, KING_ATTACKS, KNIGHT_ATTACKS, WHITE_PAWN_ATTACKS, bishop_attacks, rook_attacks}, zobrist::ZobristTable};

impl BitBoard {
    #[inline(always)]
    pub fn has_attack_to_square(&self, sq: u8, attack_is_white: bool) -> bool {
        let victim = sq as usize;
        if attack_is_white {
            let may_be_bishop_mask = bishop_attacks(victim, self.total_occupied);
            if may_be_bishop_mask & self.white[PieceType::Bishop as usize] != 0 { return true; }
            let may_be_rook_mask = rook_attacks(victim, self.total_occupied);
            if may_be_rook_mask & self.white[PieceType::Rook as usize] != 0 { return true; }
            let may_be_queen_mask = may_be_bishop_mask | may_be_rook_mask;
            if may_be_queen_mask & self.white[PieceType::Queen as usize] != 0 { return true; }
            if KNIGHT_ATTACKS[victim] & self.white[PieceType::Knight as usize] != 0 { return true; }
            if KING_ATTACKS[victim] & self.white[PieceType::King as usize] != 0 { return true; }
            if BLACK_PAWN_ATTACKS[victim] & self.white[PieceType::Pawn as usize] != 0 { return true; }
        } else {
            let may_be_bishop_mask = bishop_attacks(victim, self.total_occupied);
            if may_be_bishop_mask & self.black[PieceType::Bishop as usize] != 0 { return true; }
            let may_be_rook_mask = rook_attacks(victim, self.total_occupied);
            if may_be_rook_mask & self.black[PieceType::Rook as usize] != 0 { return true; }
            let may_be_queen_mask = may_be_bishop_mask | may_be_rook_mask;
            if may_be_queen_mask & self.black[PieceType::Queen as usize] != 0 { return true; }
            if KNIGHT_ATTACKS[victim] & self.black[PieceType::Knight as usize] != 0 { return true; }
            if KING_ATTACKS[victim] & self.black[PieceType::King as usize] != 0 { return true; }
            if WHITE_PAWN_ATTACKS[victim] & self.black[PieceType::Pawn as usize] != 0 { return true; }
        }
        false
    }

    #[inline(always)]
    pub fn is_check(&self, attack_is_white: bool) -> bool {
        let king_bitboard = if attack_is_white {
            self.black[PieceType::King as usize]
        } else {
            self.white[PieceType::King as usize]
        };
        if king_bitboard == 0 { return true; }
        let king_sq = king_bitboard.trailing_zeros() as u8;
        self.has_attack_to_square(king_sq, attack_is_white)
    }

    pub fn make_move(&mut self, mv: Move, z: &ZobristTable) {
        // [중요] 1. 아무것도 변경하기 전의 원본 상태와 해시를 History에 먼저 저장합니다.
        // 이렇게 해야 unmake_move에서 완벽하게 동일한 값으로 복구됩니다.
        self.history.push_to_tuple((mv.0, self.state, self.hash));

        let is_white = self.state_get_is_white_controll();
        
        // 2. 현재 상태의 가변 요소(Side, Castle, EP) 해시 업데이트 준비
        // Side는 매 수마다 토글됩니다.
        self.hash ^= z.side; 
        
        // 현재 권한과 EP를 해시에서 제거
        self.hash ^= z.castle[self.state_get_castling_flag() as usize];
        if let Some(pos) = self.state_get_ep_pos() {
            self.hash ^= z.en_passant[(pos & 0x07) as usize];
        }

        let from_to_mask = mv.get_from_and_to_mask();
        let (from_bit, to_bit) = from_to_mask;
        let (from_sq, to_sq) = (from_bit.trailing_zeros() as usize, to_bit.trailing_zeros() as usize);
        let moving_p_type = mv.get_moving_piece_type();
        let move_type = mv.get_move_type();

        // 3. 기물 이동 및 해시 업데이트
        let p_idx = if is_white { (moving_p_type as usize) << 1 } else { ((moving_p_type as usize) << 1) | 1 };
        
        if is_white {
            self.white[moving_p_type as usize] ^= from_bit | to_bit;
            self.white_occupied ^= from_bit | to_bit;
        } else {
            self.black[moving_p_type as usize] ^= from_bit | to_bit;
            self.black_occupied ^= from_bit | to_bit;
        }
        self.hash ^= z.pieces[p_idx][from_sq];
        self.hash ^= z.pieces[p_idx][to_sq];
        
        self.type_board[from_sq] = PieceType::None;
        self.type_board[to_sq] = moving_p_type;

        self.sub_eval_table(moving_p_type, is_white, from_sq);
        self.add_eval_table(moving_p_type, is_white, to_sq);

        // 4. 특수 이동 처리
        match move_type {
            MoveType::Quiet => {},
            MoveType::Capture => {//잡힌 기물 제거
                let cap_p_type = mv.get_captured_piece_type();
                let cap_idx = if is_white { ((cap_p_type as usize) << 1) | 1 } else { (cap_p_type as usize) << 1 };
                if is_white {
                    self.black[cap_p_type as usize] ^= to_bit;
                    self.black_occupied ^= to_bit;
                } else {
                    self.white[cap_p_type as usize] ^= to_bit;
                    self.white_occupied ^= to_bit;
                }
                self.hash ^= z.pieces[cap_idx][to_sq];
                self.sub_eval_table(cap_p_type, !is_white, to_sq);
            },
            MoveType::DoublePush => {
                // DoublePush 자체는 pieces 이동만 수행, EP 타겟 설정은 아래 5번에서 처리
            },
            MoveType::EnPassant => {//앙파상 된 폰 제거 
                let cap_sq = if is_white { to_sq - 8 } else { to_sq + 8 };
                let cap_bit = 1u64 << cap_sq;
                let cap_idx = if is_white { ((PieceType::Pawn as usize) << 1) | 1 } else { (PieceType::Pawn as usize) << 1 };
                
                if is_white {
                    self.black[PieceType::Pawn as usize] ^= cap_bit;
                    self.black_occupied ^= cap_bit;
                } else {
                    self.white[PieceType::Pawn as usize] ^= cap_bit;
                    self.white_occupied ^= cap_bit;
                }
                self.hash ^= z.pieces[cap_idx][cap_sq];
                self.type_board[cap_sq] = PieceType::None;
                self.sub_eval_table(PieceType::Pawn, !is_white, cap_sq);
            },
            MoveType::Promotion | MoveType::PromotionCapture => {//현재 기물 제거, 프로모션 기물 추가 그리고 잡힌 기물 제거
                // 도착한 폰을 해시와 비트보드에서 제거
                self.hash ^= z.pieces[p_idx][to_sq];
                self.sub_eval_table(PieceType::Pawn, is_white, to_sq);
                
                if move_type == MoveType::PromotionCapture {
                    let cap_p_type = mv.get_captured_piece_type();
                    let cap_idx = if is_white { ((cap_p_type as usize) << 1) | 1 } else { (cap_p_type as usize) << 1 };
                    if is_white {
                        self.black[cap_p_type as usize] ^= to_bit;
                        self.black_occupied ^= to_bit;
                    } else {
                        self.white[cap_p_type as usize] ^= to_bit;
                        self.white_occupied ^= to_bit;
                    }
                    self.hash ^= z.pieces[cap_idx][to_sq];
                    self.sub_eval_table(cap_p_type, !is_white, to_sq);
                }

                let pro_p_type = mv.get_promotion_piece_type();
                let pro_idx = if is_white { (pro_p_type as usize) << 1 } else { ((pro_p_type as usize) << 1) | 1 };
                if is_white {
                    self.white[PieceType::Pawn as usize] ^= to_bit;
                    self.white[pro_p_type as usize] ^= to_bit;
                } else {
                    self.black[PieceType::Pawn as usize] ^= to_bit;
                    self.black[pro_p_type as usize] ^= to_bit;
                }
                self.hash ^= z.pieces[pro_idx][to_sq];
                self.type_board[to_sq] = pro_p_type;
                self.add_eval_table(pro_p_type, is_white, to_sq);
            },
            MoveType::KingSideCastling => {//룩 이동
                let rook_mask = if is_white { 0xA0 } else { 0xA0 << 56 };
                let (r_from, r_to) = if is_white { (7, 5) } else { (63, 61) };
                let r_idx = if is_white { (PieceType::Rook as usize) << 1 } else { ((PieceType::Rook as usize) << 1) | 1 };
                
                if is_white {
                    self.white[PieceType::Rook as usize] ^= rook_mask;
                    self.white_occupied ^= rook_mask;
                } else {
                    self.black[PieceType::Rook as usize] ^= rook_mask;
                    self.black_occupied ^= rook_mask;
                }
                self.hash ^= z.pieces[r_idx][r_from];
                self.hash ^= z.pieces[r_idx][r_to];

                self.type_board[r_from] = PieceType::None;
                self.type_board[r_to] = PieceType::Rook;

                self.sub_eval_table(PieceType::Rook, is_white, r_from);
                self.add_eval_table(PieceType::Rook, is_white, r_to);
            },
            MoveType::QueenSideCastling => {
                let rook_mask = if is_white { 0x09 } else { 0x09 << 56 };
                let (r_from, r_to) = if is_white { (0, 3) } else { (56, 59) };
                let r_idx = if is_white { (PieceType::Rook as usize) << 1 } else { ((PieceType::Rook as usize) << 1) | 1 };

                if is_white {
                    self.white[PieceType::Rook as usize] ^= rook_mask;
                    self.white_occupied ^= rook_mask;
                } else {
                    self.black[PieceType::Rook as usize] ^= rook_mask;
                    self.black_occupied ^= rook_mask;
                }
                self.hash ^= z.pieces[r_idx][r_from];
                self.hash ^= z.pieces[r_idx][r_to];

                self.type_board[r_from] = PieceType::None;
                self.type_board[r_to] = PieceType::Rook;

                self.sub_eval_table(PieceType::Rook, is_white, r_from);
                self.add_eval_table(PieceType::Rook, is_white, r_to);
            }
        }

        // 5. 상태(State) 필드 업데이트
        self.state_ep_pos_clear(); // 이전 EP 타겟 제거
        if move_type == MoveType::DoublePush {
            let ep_target = if is_white { to_sq - 8 } else { to_sq + 8 };
            self.state_set_ep_pos(ep_target as u8);
        }

        // 50수 규칙용 카운터: 폰 이동 또는 캡처면 리셋, 아니면 +1
        let is_capture = matches!(move_type, MoveType::Capture | MoveType::EnPassant | MoveType::PromotionCapture);
        if moving_p_type == PieceType::Pawn || is_capture {
            self.state_reset_half_move();
        } else {
            self.state_increment_half_move();
        }
        // 흑이 둔 뒤 full move 증가
        if !is_white {
            self.state_increment_full_move();
        }
        
        self.state_lose_castling_right_by_bit(CASTLING_MASK[from_sq]);
        self.state_lose_castling_right_by_bit(CASTLING_MASK[to_sq]);
        self.state_set_controll_change(); // Side 변경 (차례 전환)

        // 6. 새로운 상태의 가변 요소 해시 적용
        self.hash ^= z.castle[self.state_get_castling_flag() as usize];
        if let Some(pos) = self.state_get_ep_pos() {
            self.hash ^= z.en_passant[(pos & 0x07) as usize];
        }

        self.total_occupied = self.white_occupied | self.black_occupied;
    }

    /// Null move pruning: 기물을 움직이지 않고 차례만 넘김
    pub fn make_null_move(&mut self, z: &ZobristTable) {
        // move 자리에 0을 저장 → 연속 null move 판별에 사용
        self.history.push_to_tuple((0, self.state, self.hash));

        if let Some(pos) = self.state_get_ep_pos() {
            self.hash ^= z.en_passant[(pos & 0x07) as usize];
        }
        self.state_ep_pos_clear();
        self.state_reset_half_move(); // null move를 넘어 반복 판정이 이어지지 않도록
        self.state_set_controll_change();
        self.hash ^= z.side;
    }

    pub fn unmake_null_move(&mut self) {
        let (_, prev_state, prev_hash) = self.history.pop_to_tuple();
        self.state = prev_state;
        self.hash = prev_hash;
    }

    /// 현재 차례인 쪽에 폰/킹 외의 기물이 있는지
    #[inline(always)]
    pub fn has_non_pawn_material(&self) -> bool {
        let side = if self.state_get_is_white_controll() { &self.white } else { &self.black };
        (side[PieceType::Knight as usize] | side[PieceType::Bishop as usize]
            | side[PieceType::Rook as usize] | side[PieceType::Queen as usize]) != 0
    }

    pub fn unmake_move(&mut self) {
        // 1. History에서 이전 상태(State), 해시(Hash), 그리고 방금 수행한 이동 데이터를 복구합니다.
        let (mv_data, prev_state, prev_hash) = self.history.pop_to_tuple();
        let mv = Move(mv_data, 0);
        
        // 2. 보드 상태와 해시를 즉시 이전 시점으로 되돌립니다.
        self.state = prev_state;
        self.hash = prev_hash;
        
        // 중요: state를 먼저 복구했으므로, state_get_is_white_controll()은 
        // '수를 두기 전의 색상'(현재 복구된 차례)을 반환합니다.
        let is_white = self.state_get_is_white_controll();
        
        let from_to_mask = mv.get_from_and_to_mask();
        let (from_bit, to_bit) = from_to_mask;
        let from_sq = from_bit.trailing_zeros() as usize;
        let to_sq = to_bit.trailing_zeros() as usize;
        
        let moving_p_type = mv.get_moving_piece_type();
        let move_type = mv.get_move_type();

        // 3. 비트보드 위치 복구 (기본 이동 처리)
        // 프로모션이 아닌 경우 moving_p_type이 to에서 from으로 이동합니다.
        if is_white {
            self.white[moving_p_type as usize] ^= from_bit | to_bit;
            self.white_occupied ^= from_bit | to_bit;
        } else {
            self.black[moving_p_type as usize] ^= from_bit | to_bit;
            self.black_occupied ^= from_bit | to_bit;
        }

        // 4. 이동 타입별 상세 복구 로직 (평가 점수 및 특수 기물)
        match move_type {
            MoveType::Promotion | MoveType::PromotionCapture => {
                // [버그 수정]: 프로모션 시 to_sq에는 변환된 기물이, from_sq에는 폰이 있어야 함
                let pro_p_type = mv.get_promotion_piece_type();
                
                // 1) 비트보드 수정: 위에서 처리한 moving_p_type(폰)의 to_bit을 취소하고 프로모션 기물을 제거
                if is_white {
                    self.white[PieceType::Pawn as usize] ^= to_bit; // to_bit 제거 (이미 XOR되어 빠진 상태를 다시 XOR하여 0으로 만듦)
                    self.white[pro_p_type as usize] ^= to_bit;      // 프로모션 기물 비트 제거
                } else {
                    self.black[PieceType::Pawn as usize] ^= to_bit;
                    self.black[pro_p_type as usize] ^= to_bit;
                }

                // 2) 평가값 수정: 프로모션 기물 점수 제거, 폰 점수 추가
                self.sub_eval_table(pro_p_type, is_white, to_sq);
                self.add_eval_table(PieceType::Pawn, is_white, from_sq);

                // 3) 타입 보드 수정
                self.type_board[from_sq] = PieceType::Pawn;
                
                if move_type == MoveType::PromotionCapture {
                    let cap_p_type = mv.get_captured_piece_type();
                    self.restore_captured_piece(to_sq, to_bit, cap_p_type, !is_white);
                } else {
                    self.type_board[to_sq] = PieceType::None;
                }
            },

            MoveType::Capture => {
                self.type_board[from_sq] = moving_p_type;
                self.sub_eval_table(moving_p_type, is_white, to_sq);
                self.add_eval_table(moving_p_type, is_white, from_sq);

                let cap_p_type = mv.get_captured_piece_type();
                self.restore_captured_piece(to_sq, to_bit, cap_p_type, !is_white);
            },

            MoveType::EnPassant => {
                self.type_board[from_sq] = PieceType::Pawn;
                self.type_board[to_sq] = PieceType::None;
                self.sub_eval_table(PieceType::Pawn, is_white, to_sq);
                self.add_eval_table(PieceType::Pawn, is_white, from_sq);

                // 잡힌 폰의 위치 계산 및 복구
                let cap_sq = if is_white { to_sq - 8 } else { to_sq + 8 };
                let cap_bit = 1u64 << cap_sq;
                self.restore_captured_piece(cap_sq, cap_bit, PieceType::Pawn, !is_white);
            },

            MoveType::KingSideCastling => {
                self.type_board[from_sq] = PieceType::King;
                self.type_board[to_sq] = PieceType::None;
                self.sub_eval_table(PieceType::King, is_white, to_sq);
                self.add_eval_table(PieceType::King, is_white, from_sq);

                // 룩 복구
                let (r_from, r_to) = if is_white { (7, 5) } else { (63, 61) };
                self.move_rook_for_castling(r_from, r_to, is_white);
            },

            MoveType::QueenSideCastling => {
                self.type_board[from_sq] = PieceType::King;
                self.type_board[to_sq] = PieceType::None;
                self.sub_eval_table(PieceType::King, is_white, to_sq);
                self.add_eval_table(PieceType::King, is_white, from_sq);

                // 룩 복구
                let (r_from, r_to) = if is_white { (0, 3) } else { (56, 59) };
                self.move_rook_for_castling(r_from, r_to, is_white);
            },

            _ => { // Quiet, DoublePush
                self.type_board[from_sq] = moving_p_type;
                self.type_board[to_sq] = PieceType::None;
                self.sub_eval_table(moving_p_type, is_white, to_sq);
                self.add_eval_table(moving_p_type, is_white, from_sq);
            }
        }

        // 5. 전체 점유 보드 갱신
        self.total_occupied = self.white_occupied | self.black_occupied;
    }

    /// 잡힌 기물을 비트보드, 타입보드, 평가값에 복구하는 헬퍼 함수
    #[inline(always)]
    fn restore_captured_piece(&mut self, sq: usize, bit: u64, p_type: PieceType, is_white: bool) {
        if is_white {
            self.white[p_type as usize] |= bit;
            self.white_occupied |= bit;
        } else {
            self.black[p_type as usize] |= bit;
            self.black_occupied |= bit;
        }
        self.type_board[sq] = p_type;
        self.add_eval_table(p_type, is_white, sq);
    }

    /// 캐슬링 시 룩의 위치를 되돌리고 평가값을 갱신하는 헬퍼 함수
    #[inline(always)]
    fn move_rook_for_castling(&mut self, r_from: usize, r_to: usize, is_white: bool) {
        let r_bit_mask = (1u64 << r_from) | (1u64 << r_to);
        if is_white {
            self.white[PieceType::Rook as usize] ^= r_bit_mask;
            self.white_occupied ^= r_bit_mask;
        } else {
            self.black[PieceType::Rook as usize] ^= r_bit_mask;
            self.black_occupied ^= r_bit_mask;
        }
        self.type_board[r_from] = PieceType::Rook;
        self.type_board[r_to] = PieceType::None;
        self.sub_eval_table(PieceType::Rook, is_white, r_to);
        self.add_eval_table(PieceType::Rook, is_white, r_from);
    }
}

static CASTLING_MASK: [u8; 64] = {
    let mut masks = [0u8; 64];
    masks[7]  = 0b1000; // h1
    masks[0]  = 0b0100; // a1
    masks[4]  = 0b1100; // e1
    masks[63] = 0b0010; // h8
    masks[56] = 0b0001; // a8
    masks[60] = 0b0011; // e8
    masks
};