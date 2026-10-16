const PIECE_VALUE:[i32;6] = [100, 300, 300, 500, 900, 2000];
const PICE_TYPE_FOR_INDEX:[PieceType;6] = 
[PieceType::Pawn, PieceType::Knight,PieceType::Bishop, PieceType::Rook, PieceType::Queen, PieceType::King];

#[repr(u8)]
#[derive(Clone,Copy,Debug,PartialEq)]
pub enum PieceType {
    Pawn =0,
    Knight =1,
    Bishop =2,
    Rook =3,
    Queen =4,
    King =5,
    None =6,
}

impl PieceType {
    #[inline(always)]
    pub fn u8_as_piece_type(t:u8) -> PieceType{
        if t >=6{
            return PieceType::None
        }
        PICE_TYPE_FOR_INDEX[t as usize]

    }

    #[inline(always)]
    pub fn get_value(&self) -> i32 {
        PIECE_VALUE[*self as usize]
    }

    pub fn to_char(&self) -> char{
        match self {
            PieceType::Bishop => 'b',
            PieceType::King => 'K',
            PieceType::Knight => 'N',
            PieceType::Pawn => 'P',
            PieceType::Queen => 'Q',
            PieceType::Rook => 'R',
            PieceType::None => 'N'
        }
    }
}
