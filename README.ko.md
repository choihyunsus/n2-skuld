[English](README.md)

# n2-skuld

소스의 미완성 표시 탐색, 지식 최신성 계산, Markdown 보고서, Soul 공통 SQLite 아카이브를 제공하는 Rust 라이브러리와 JSON 명령행 도구입니다.

## 설치와 실행

Rust 1.98 이상과 SQLite·Tree-sitter를 빌드할 C/C++ 도구가 필요합니다.

```sh
cargo install --git https://github.com/choihyunsus/n2-skuld --tag v1.0.0 --locked
n2-skuld scan ./src --project demo --agent worker --db ./mimir.db --pages ./skuld-pages
n2-skuld page ./skuld-pages todo
```

소스 체크아웃에서는 `cargo build --locked --release`로 빌드합니다. `--db`와 `--pages`는 선택 사항입니다. 저장 요청 시 프로젝트·에이전트 이름과 함께 보고서를 저장합니다.

## 소스 검사

Rust, TypeScript/TSX, JavaScript/JSX, Python, Go, Java, C, C++, Bash 문법을 파싱합니다. `.zsh` 파일에도 Bash 문법을 적용합니다. 주석의 TODO·FIXME·XXX·HACK·BUG와 Rust의 미구현 매크로를 찾고, 문자열 안에 적힌 표시는 제외합니다.

기본 한도는 소스 5,000개, 총 64 MiB, 파일당 1 MiB, 디렉터리 깊이 64, 결과 10,000개, 전체 30초입니다. 파일별 파싱과 구문 트리 탐색은 1초로 제한하며 디렉터리 항목 50,000개에서도 중단합니다. 숨김·빌드 폴더, 심볼릭 링크와 Windows 재분석 지점을 건너뜁니다. 소스는 UTF-8이어야 합니다.

구문 오류, 읽기 실패와 한도 도달은 `issues`에 기록하며 `complete`가 false로 표시됩니다. 부분 검사 결과를 출력한 뒤 종료 코드 2를 반환합니다. 실행 오류의 종료 코드는 1입니다.

페이지는 UUID별 새 폴더에 완성한 뒤 `current.json`을 교체해 공개합니다. 읽기는 현재 매니페스트를 따릅니다. 동시에 쓰는 작업은 파일 잠금으로 제한하고, 부분 검사 표시와 기존 사용자 메모를 보존합니다. 이전 세대의 정리 정책은 호스트가 관리합니다. Unix에서는 디렉터리 메타데이터도 동기화하며 Windows에서는 파일시스템의 이름 변경 동작을 사용합니다.

## Soul 공통 저장소

배포판 Soul과 Mimir가 사용하는 `mimir.db`를 `--db`에 지정합니다. 유지보수 전에 DB를 백업하세요.

```sh
n2-skuld init --db ./mimir.db
```

공통 9개 테이블의 필드를 사용합니다. `experiences`, `insights`, `tags`, `embeddings`, `effect_tracking`, `experience_insight_links`, `insight_links`, `meta`, `tag_similarity`가 해당됩니다. `skuld_scans`에는 프로젝트·에이전트·검사 경로·완료 여부·JSON 보고서와 생성 시각을 저장합니다. 버전 및 저장소 UUID는 `meta`에 기록합니다.

기존 Soul의 호환 스키마는 버전 표기가 없어도 읽을 수 있습니다. 미래 스키마 버전은 거부합니다. 별도의 대붕이 전용 테이블은 명시적인 이전 작업이 필요합니다.

## 아카이브와 회상

```sh
n2-skuld archive --db ./mimir.db --archive ./archive.db --project demo --agent worker --retention-days 90 --limit 1000
n2-skuld recall --db ./mimir.db --archive ./archive.db --project demo --agent worker --query "database retry" --limit 20
```

아카이브는 명시적으로 실행하는 유지보수입니다. UTC `created_at`을 기준으로 오래된 경험을 고르며, 경험의 전체 필드와 태그·벡터·모델 정보를 복사합니다. 다른 테이블이 외래키로 참조하는 근거는 활성 DB에 유지합니다. 호스트의 추가 논리 참조에도 외래키를 선언해야 이 보호가 적용됩니다.

복사본을 먼저 커밋한 뒤 내용과 참조가 그대로인 활성 행만 제거합니다. 중단되면 양쪽에 복사본이 남을 수 있으며 재시도할 수 있습니다. 복사 뒤 변경되거나 새 참조가 생긴 행은 유지합니다. 아카이브는 원본 DB의 UUID와 결합하며 동일 파일·하드링크·다른 원본의 아카이브를 거부합니다.

Soul과 Mimir의 일반 검색은 활성 DB를 사용합니다. 보관한 경험까지 필요할 때 Skuld의 회상을 호출합니다. 회상은 아카이브를 읽기 전용으로 열고, 출처 ID·계층·주체·최신성 점수를 돌려줍니다. 활성 데이터가 변경되어 검색어와 맞지 않게 된 경우에도 이전 아카이브 복사본보다 활성 상태를 우선합니다.

경험은 유니코드 FTS 단어로, 활성 통찰은 리터럴 부분 문자열로 검색합니다. 통찰은 같은 주체의 global/agent 범위 또는 출처로 확인한 프로젝트 범위를 사용합니다. 출처별 후보 200개와 결과 상한이 있으며 잘림 여부를 반환합니다. 출처 안의 순위 역수에 최신성을 곱해 정렬합니다. 결과의 현재 타당성은 호출자가 검토합니다.

묶음당 최대 경험 1,000개를 처리합니다. 최소 SQLite 3.51.3을 검사하며 고정 의존성은 3.54.1을 포함합니다. 공통 DB를 쓰는 모든 프로그램에 WAL-reset 수정이 포함된 엔진을 사용하세요. 활성 DB와 아카이브를 함께 백업하세요. [SQLite 수정 내역](https://www.sqlite.org/releaselog/3_51_3.html)

## 최신성과 의미 연결

```sh
n2-skuld freshness "2026-01-01T00:00:00Z" --category library_api
n2-skuld prune --db ./mimir.db --archive ./archive.db --project demo --agent worker --keep 10 --limit 1000
```

최신성은 `1 / (1 + 감쇠율 × 경과일)`로 계산합니다. 아키텍처 0.002, 코드 패턴 0.005, 보안 0.008, 문서·일반 지식 0.01, 라이브러리 API 0.02를 적용합니다. 시간대 없는 값은 UTC로 해석하고 미래 날짜는 1, 잘못된 날짜는 0을 반환합니다. SQL 표현식 생성에도 허용된 열 이름을 검사합니다.

의미 연결 정리는 주체별 `verdandi_semantic_edges` 테이블을 사용합니다. 필드는 `project`, `agent`, `source_path`, `target_path`, `similarity`, `edge_type`, `model`, `computed_at`입니다. 라이브러리의 `initialize_semantic_edges`가 버전 1 구조를 만들며, CLI의 `prune`은 해당 구조가 존재해야 동작합니다. 이후 Verdandi 연동도 이 공개 계약을 사용합니다.

파일별 상위 연결을 유지하고 나머지를 먼저 복사한 뒤 순위·내용을 재확인해 제거합니다. 유사도가 같으면 대상 경로로 순서를 고정합니다. 예전 대붕이의 주체 없는 테이블은 명시적인 소유 범위 이전이 필요합니다.

## 검증과 사용 경계

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo package --locked
```

구문 분석, 예산·부분 결과, 실제 CLI, 범위별 저장, 메타데이터 보존, 복사 후 삭제 실패, 두 커밋 사이 변경, 외래키 보호, 아카이브 결합, 최신성 SQL과 페이지 공개를 검증합니다. Soul 9.2.1이 만든 DB를 실제 CLI로 처리하고 Mimir 3.0.1과 공통 필드가 맞는지도 확인했습니다.

Windows·Rust 1.98에서 검증합니다. 다른 운영체제의 네이티브 빌드는 해당 환경에서 확인해야 합니다. 일정 실행, MCP 도구 등록과 인증은 호스트가 연결합니다. 프로젝트·에이전트 이름은 신뢰할 수 있는 로컬 저장소 안의 데이터 범위를 식별합니다. 아카이브 작업에는 전용 연결을 사용하세요.

MIT 라이선스이며 출처와 의존성 안내는 `THIRD-PARTY-NOTICES`에 있습니다. Rust 사용 예시는 영문 README를 참고하세요.
