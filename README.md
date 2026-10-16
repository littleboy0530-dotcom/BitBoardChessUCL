# BitBoardChess UCI

Rust로 작성한 비트보드 기반 체스 엔진입니다. Godot 체스 게임(`bitboardchess`)용으로 만든 엔진 코어(비트보드, 수 생성, 평가, 탐색)를
UCI(Universal Chess Interface) 프로토콜로 동작하는 독립 콘솔 엔진으로 옮긴 프로젝트로,
Arena, Cute Chess, BanksiaGUI 등 UCI를 지원하는 GUI에 등록해 대국할 수 있습니다.

- 작성자: Joon
- 언어: Rust (edition 2021), 외부 의존성: `rand`

## 기력

Stockfish 19의 기력 제한 모드(`UCI_LimitStrength` / `UCI_Elo`)를 상대로 측정한 결과입니다.

| 상대 | 결과 (승-패-무) | 승률 |
|---|---|---|
| Stockfish 1800 | 31-8-1 | 78.7% |
| Stockfish 2100 | 24-14-2 | 62.5% |
| Stockfish 2400 | 19-16-5 | 53.8% |

**추정 레이팅: 약 2200 (95% 신뢰구간 ±80)**

- 측정 조건: cutechess-cli 1.5.1, 레벨당 40판(총 120판), 10초+0.1초, 1스레드, 30개 오프닝을 흑백 번갈아 사용
- 상대 시간 초과 패 12판을 제외하고 최대우도법으로 합산한 값 (포함 시 약 2257)
- Stockfish `UCI_Elo` 척도(CCRL 계열) 기준이며 사람 레이팅(FIDE, chess.com)과는 직접 비교할 수 없습니다

## 빌드

```
cargo build --release
```

실행 파일: `target/release/bitboard_chess_uci.exe`

release 프로필은 `opt-level = 3`, `lto = true`, `codegen-units = 1`, `panic = "abort"`로 설정되어 있습니다.
`src/modules/tables.rs`(매직 비트보드 공격 테이블, 약 9MB)가 커서 첫 빌드는 시간이 조금 걸립니다.

## 사용법

### GUI에 등록
GUI의 엔진 추가 메뉴에서 `bitboard_chess_uci.exe`를 선택하고 프로토콜을 **UCI**로 지정하면 됩니다.

### 콘솔에서 직접 사용
```
uci
setoption name MultiPV value 3
position startpos moves e2e4 e7e5
go movetime 2000
```

출력 예:
```
info depth 9 multipv 1 score cp 7 nodes 4285858 nps 6053471 time 708 pv f8d6 b1c3 g8f6 ...
bestmove f8d6 ponder b1c3
```

### UCI 옵션

| 옵션 | 기본값 | 범위 | 설명 |
|---|---|---|---|
| `Threads` | 1 | 1~256 | 탐색 스레드 수 (Lazy SMP) |
| `MultiPV` | 1 | 1~256 | 출력할 최선 수 개수 |

```
setoption name Threads value 4
setoption name MultiPV value 3
```

### 지원 명령

| 명령 | 설명 |
|---|---|
| `uci` | 엔진 정보와 옵션 출력 후 `uciok` |
| `isready` | `readyok` 응답 |
| `setoption name <이름> value <값>` | 옵션 설정 |
| `ucinewgame` | 전치 테이블 초기화, 시작 국면으로 리셋 |
| `position startpos [moves ...]` | 시작 국면 + 수순 |
| `position fen <FEN> [moves ...]` | FEN 국면 + 수순 |
| `go depth N` | 깊이 N까지 탐색 |
| `go movetime ms` | 지정 시간(ms) 동안 탐색 |
| `go wtime/btime/winc/binc/movestogo` | 대국 시계 기준 시간 배분 |
| `go infinite` | `stop`을 받을 때까지 탐색 |
| `go perft N` | 깊이 N 수 생성 노드 수 (디버그용) |
| `go` | 제한 없이 보내면 깊이 10으로 탐색 |
| `stop` | 탐색 중단 후 `bestmove` 출력 |
| `d` | 현재 보드와 3회 동형 여부 출력 (디버그용) |
| `quit` | 종료 |

## 엔진에 적용된 기술

### 보드 표현
- **비트보드**: 백/흑 각각 기물 종류별 `u64` 6개 + 백/흑/전체 점유 비트보드. 칸 번호는 a1 = 0 … h8 = 63
- **타입 보드**: 칸별 기물 종류 배열(`[PieceType; 64]`)을 함께 유지해 잡힌 기물 종류를 O(1)로 조회
- **상태 비트 패킹**: 차례, 앙파상 플래그/칸, 캐슬링 권리(4비트), 50수 카운터, 전체 수 번호를 `u32` 하나에 저장

### 수 생성
- **매직 비트보드**: 룩/비숍(퀸) 슬라이딩 공격을 미리 계산한 매직 넘버 테이블로 곱셈 한 번에 조회
- **미리 계산된 공격 테이블**: 나이트, 킹, 폰 공격
- **비트 연산 기반 폰 생성**: 전진/더블 푸시/좌우 캡처/프로모션(4종)/앙파상을 시프트 연산으로 일괄 생성
- **캐슬링**: 권리, 사이 칸 비어 있음, 체크 상태 및 통과 칸 공격 여부를 확인
- **유사 합법수 생성 + 합법성 검사**: 수를 둔 뒤 자기 킹이 공격받는지로 걸러냄
- **32비트 수 인코딩**: 출발/도착 칸, 이동 기물, 잡은 기물, 승격 기물, 수 종류(일반/캡처/더블 푸시/앙파상/승격/승격 캡처/킹사이드/퀸사이드 캐슬링)를 하나의 `u32`에 저장
- perft로 검증: 시작 국면, Kiwipete, Position 3/4/5 표준 테스트 국면의 노드 수가 모두 기준값과 일치

### Make / Unmake
- 수를 둘 때 이전 상태와 해시를 히스토리 스택에 저장하고, 되돌릴 때 그대로 복원
- 비트보드, 타입 보드, 해시, 평가값을 모두 **증분 업데이트**

### 해싱
- **Zobrist 해싱**: 기물×칸, 차례, 캐슬링 권리(16가지), 앙파상 파일에 난수를 배정하고 수마다 XOR로 증분 갱신

### 평가
- **PeSTO 기물-칸 테이블(PST)**: 중반(MG)/종반(EG) 기물 가치와 칸별 보너스
- **테이퍼드 평가(Tapered Eval)**: 남은 기물로 게임 단계(0~24)를 계산해 중반/종반 점수를 보간
- **증분 평가**: 수를 둘 때마다 점수를 더하고 빼서 평가 비용을 최소화

### 탐색
- **네가맥스 + 알파-베타 가지치기**
- **반복 심화(Iterative Deepening)**: 깊이 1부터 늘려 가며 탐색, 메이트를 찾으면 조기 종료
- **전치 테이블(TT)**: 2^24 엔트리(256MB), 정확값/하한/상한 플래그, 깊이 우선 교체
- **Null Move Pruning**: 깊이 3 이상, 체크 아님, 폰 외 기물이 있을 때 R = 2~3으로 적용 (연속 null move 금지)
- **수 정렬**: TT 수 → 캡처(MVV-LVA) → 킬러 무브 → 나머지
- **킬러 무브 휴리스틱**: 베타 컷을 일으킨 조용한 수를 ply별로 기록
- **정적 탐색(Quiescence Search)**: 리프에서 캡처가 끝날 때까지 추가 탐색해 지평선 효과 완화
- **메이트 거리 점수**: 더 빠른 메이트를 선호
- **3회 동형 무승부**: 탐색 트리 안에서 국면이 반복되면 0점, 실제 대국 기록의 국면과만 겹치면 3회째에 0점

### UCI 기능
- **시간 관리**: `movetime` 또는 남은 시간 / 남은 수(기본 30) + 증가 시간의 3/4로 배분, 남은 시간의 절반을 상한으로 두고 통신 지연 30ms를 뺌. 2048노드마다 시간을 확인해 초과 시 즉시 중단하고, 중단된 깊이의 결과는 버림
- **MultiPV**: 루트에서 이미 찾은 수를 제외하며 반복 탐색해 상위 N개 수와 점수를 출력
- **PV 추출**: TT를 따라가며 주요 변화를 구성 (합법성/반복 검사 포함)
- **멀티스레드 (Lazy SMP)**: 보조 스레드가 루트 수 순서와 시작 깊이를 달리해 같은 국면을 탐색하며 공유 TT를 채움
- **Lock-free TT**: 엔트리를 `(key ^ data, data)` 두 개의 `AtomicU64`로 저장해 동시 쓰기로 값이 섞이면 key 검증에서 걸러냄
- 탐색은 별도 스레드에서 실행되어 탐색 중에도 `stop`/`quit`에 즉시 반응

## 프로젝트 구조

```
src/
├── main.rs              UCI 명령 처리, FEN 파싱, 시간 관리, 탐색 스레드, perft
├── modules.rs
└── modules/
    ├── bitboard.rs      보드 구조, 상태 비트 패킹, 히스토리, 반복 판정
    ├── move_.rs         수 인코딩, 수 생성, UCI 표기 변환
    ├── simulation.rs    make/unmake, null move, 공격/체크 판정
    ├── eval.rs          PeSTO 테이블, 테이퍼드 평가
    ├── search.rs        네가맥스, 정적 탐색, 반복 심화, MultiPV, 탐색 제어
    ├── zobrist.rs       Zobrist 키, lock-free 전치 테이블
    ├── piece.rs         기물 종류와 가치
    └── tables.rs        매직 비트보드 / 공격 테이블 (자동 생성)
```

## 레이팅 측정 환경

`E:\MyDocuments\chess_tools`에 cutechess-cli 1.5.1, Stockfish 19, 측정 스크립트가 있습니다.

```
# 레벨당 판 수, Stockfish 레벨...
/e/MyDocuments/chess_tools/run_gauntlet.sh 40 2200 2400
```

엔진을 수정한 뒤에는 새로 빌드한 exe를 `chess_tools/engines_bbc.exe`로 복사한 다음 측정합니다.
결과 로그와 기보는 `chess_tools/results/`에 저장됩니다.

## 알려진 한계 / 개선 여지

- 50수 규칙 무승부는 탐색에서 판정하지 않음 (반수 카운터는 유지)
- 평가가 PST만 사용: 폰 구조, 킹 안전, 기동력 등의 항목 없음
- 킬러 무브 정렬에서 두 번째 슬롯이 사용되지 않음 (`search.rs`의 조건이 두 번 모두 `killers[ply][0]`을 비교)
- 전치 테이블 크기가 256MB로 고정 (`Hash` 옵션 없음)
- Lazy SMP의 이득이 아직 작음: 4스레드에서 NPS는 약 2배지만 도달 깊이 향상은 제한적
- 히스토리 버퍼가 1024 ply라서 매우 긴 대국(약 1000 ply 이상)은 지원하지 않음
- Ponder(상대 시간 동안 생각하기) 미지원
