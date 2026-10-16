use rand::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

// --- 전치 테이블 엔트리 ---
#[derive(Clone, Copy, Debug)]
pub struct TtEntry {
    pub hash_key: u64,
    pub best_move: u32,
    pub score: i32,
    pub depth: u8,
    pub flag: TtFlag,
    //pub repeat: (u8,u8),
}

impl TtEntry {
    #[inline(always)]
    pub fn default() -> Self {
        TtEntry { hash_key: 0, best_move: 0, score: 0, depth: 0, flag: TtFlag::Exact,}
    }
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum TtFlag {
    Exact,
    LowerBound,
    UpperBound,
}

// --- Zobrist Table (보드 해싱용) ---
#[derive(Debug)]
pub struct ZobristTable {
    pub pieces: [[u64; 64]; 12],
    pub side: u64,
    pub castle: [u64; 16],
    pub en_passant: [u64; 8],
}

impl ZobristTable {
    #[inline(always)]
    pub fn init() -> Self {
        let mut rng = rand::rng();
        let mut z = ZobristTable {
            pieces: [[0; 64]; 12],
            side: rng.random(),
            castle: [0; 16],
            en_passant: [0; 8],
        };

        for i in 0..12 {
            for j in 0..64 { z.pieces[i][j] = rng.random(); }
        }
        for i in 0..16 { z.castle[i] = rng.random(); }
        for i in 0..8 { z.en_passant[i] = rng.random(); }

        z
    }
}

// --- Transposition Table (Lock-free, 멀티스레드 공유) ---
// 엔트리는 (key ^ data, data) 두 개의 AtomicU64로 저장한다.
// 다른 스레드가 동시에 쓰는 도중 읽어서 값이 섞이면 key 검증에 실패하므로 miss로 처리된다.
struct TtSlot {
    key: AtomicU64,
    data: AtomicU64,
}

pub struct TranspositionTable {
    table: Box<[TtSlot]>,
}

// data 레이아웃: (flag:2)(depth:8)(score+SCORE_OFFSET:22)(best_move:32)
const SCORE_OFFSET: i64 = 1 << 21;

#[inline(always)]
fn pack(entry: &TtEntry) -> u64 {
    let score = (entry.score as i64 + SCORE_OFFSET).clamp(0, (1 << 22) - 1) as u64;
    (entry.best_move as u64)
        | (score << 32)
        | ((entry.depth as u64) << 54)
        | ((entry.flag as u64) << 62)
}

#[inline(always)]
fn unpack(hash_key: u64, data: u64) -> TtEntry {
    let flag = match data >> 62 {
        0 => TtFlag::Exact,
        1 => TtFlag::LowerBound,
        _ => TtFlag::UpperBound,
    };
    TtEntry {
        hash_key,
        best_move: data as u32,
        score: (((data >> 32) & ((1 << 22) - 1)) as i64 - SCORE_OFFSET) as i32,
        depth: ((data >> 54) & 0xFF) as u8,
        flag,
    }
}

impl TranspositionTable {
    pub fn new() -> Self {
        let size = 1 << TT_SIZE;
        let mut v = Vec::with_capacity(size);
        for _ in 0..size {
            v.push(TtSlot { key: AtomicU64::new(0), data: AtomicU64::new(0) });
        }
        Self { table: v.into_boxed_slice() }
    }

    pub fn global() -> &'static Self {
        static TT_INSTANCE: OnceLock<TranspositionTable> = OnceLock::new();
        TT_INSTANCE.get_or_init(|| Self::new())
    }

    #[inline(always)]
    pub fn get_entry(&self, hash: u64) -> TtEntry {
        let mask = (1 << TT_SIZE) - 1;
        let slot = &self.table[(hash as usize) & mask];
        let data = slot.data.load(Ordering::Relaxed);
        let key = slot.key.load(Ordering::Relaxed) ^ data;
        if key == 0 { TtEntry::default() } else { unpack(key, data) }
    }

    pub fn store_entry(&self, hash: u64, mut entry: TtEntry) {
        let mask = (1 << TT_SIZE) - 1;
        let slot = &self.table[(hash as usize) & mask];
        let old_data = slot.data.load(Ordering::Relaxed);
        let old_key = slot.key.load(Ordering::Relaxed) ^ old_data;
        let old_depth = ((old_data >> 54) & 0xFF) as u8;

        // 1. 빈 슬롯이거나
        // 2. 새로운 데이터가 더 깊게 탐색된 것이라면 덮어씀
        if old_key == 0 || entry.depth >= old_depth {
            entry.hash_key = hash;
            let data = pack(&entry);
            slot.key.store(hash ^ data, Ordering::Relaxed);
            slot.data.store(data, Ordering::Relaxed);
        }
    }

    /// 새 게임 시작 시 테이블 초기화 (탐색 중이 아닐 때만 호출)
    pub fn clear(&self) {
        for slot in self.table.iter() {
            slot.key.store(0, Ordering::Relaxed);
            slot.data.store(0, Ordering::Relaxed);
        }
    }
}

pub const TT_SIZE: i32 = 24;
