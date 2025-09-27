Rust + imgui 네이티브 이미지 뷰어

사용 라이브러리는 
dear-imgui(https://crates.io/crates/dear-imgui) 와 dear-imgui-glow를 백엔드로 하며 기타 관련 라이브러리를 추가로 사용할 수 있습니다

⸻

1) 제품 개요 / 목표
    *	목표: 가볍고 빠른 로컬 이미지 뷰어. 폴더 단위 탐색, 즉각적인 Zoom/Pan, 슬라이드쇼, 다중 포맷을 안정적으로 처리.
    *	핵심 가치:
    *	즉시성(Instant UI): 수만 장 폴더도 빠른 썸네일/프리뷰.
    *	견고성(Robust): 손상 파일/대용량 이미지에도 다운 없이 Fail-safe.
    *	일관성(Consistent UX): 마우스/트랙패드/키보드 제스처 통일.

⸻

2) 사용자 시나리오 (핵심 플로우)
    1.	앱 실행 → 마지막 열었던 폴더 자동 복원 (옵션)
    2.	좌측 Folder/Thumbnail Pane, 우측 Viewer
    3.	화살표/마우스휠로 이전/다음, Space로 Slideshow 시작/정지
    4.	Zoom/Pan: 더블클릭 100%↔Window-fit, Ctrl+휠 (macOS: ⌘+휠)
    5.	파일 정보 Overlay: 해상도, 용량, 포맷, EXIF 일부
    6.	단축키 도움말: ?로 토글

⸻

3) 기능 요구사항 (Functional Requirements)

3.1 파일/포맷
    *	지원 포맷(v1): PNG, JPEG (Progressive 포함), BMP, GIF(정지/애니 판별 후 첫 프레임만 v1), WebP(정지), TIFF(압축 없는 단일 페이지 중심)
    *	폴더 탐색: 드래그&드롭 폴더/파일, 최근 열기, 파일 정렬(이름/시간/크기/확장자, ASC/DESC)
    *	파일 연계: OS file association(더블클릭으로 앱 열기) — v1은 선택적

3.2 뷰어/조작
    *	Zoom: 12.5%~1600% (단계/연속), 100%/Fit to window/Fill 모드
    *	Pan: Middle-drag / Space-drag / 트랙패드 제스처
    *	Rotate: 90° 단위 회전(메타데이터 기반 자동회전 옵션)
    *	Background: 체크무늬/단색, 밝기 단계
    *	슬라이드쇼: 간격 1~10초, Shuffle/Loop 옵션

3.3 썸네일/그리드
    *	비동기 로딩, 품질 단계(저해상→고해상 순차 교체), LRU 캐시
    *	썸네일 크기 슬라이더(XXS~XL), Grid/Filmstrip 토글

3.4 정보/메타데이터
    *	파일 정보: 포맷/용량/경로/수정일
    *	EXIF 핵심: Orientation, DateTimeOriginal, Camera/Len(가능 시)
    *	색상 정보: 색심도, 알파 채널 여부

3.5 설정/단축키
    *	키맵 프리셋(Windows/macOS) + 사용자 커스텀(향후)
    *	성능 옵션: 디코딩 스레드 수, 캐시 용량, 프리로드 활성/비활성

⸻

4) 비기능 요구사항 (Non-Functional)
    * 안정성: 손상 파일/미지원 포맷 Graceful fallback (에러 카드 + 계속 탐색 가능)
    * 이식성: Windows 10+, macOS 12+, Ubuntu 22.04+ (Wayland/X11)
    * 보안/프라이버시: 로컬 전용, 네트워크 통신 없음(업데이트 체크 옵션 제외)
    * 알기 쉬운 코드와 적절히 객체로 분리되어 작성된 코드
    * 주석은 Easy English로 작성

⸻

5) 기술 스택 / 아키텍처

5.1 UI/런타임
    *	egui + eframe(WGPU 백엔드 우선), winit(플랫폼 이벤트)
    *	텍스처 업로드/리사이즈는 GPU 경로 우선, 일부 소프트 경로 병행

5.2 이미지 파이프라인
    *	Decoder Layer: 포맷별 디코딩 crate(예: image 계열) 추상화 인터페이스
    *	Processor Layer: 리사이즈(고속 선형/박스), 타일 생성, Mipmap-like Pyramid 캐시
    *	Cache Layer:
    *	Memory LRU: Original/Downscaled/Tile
    *	Disk Cache(옵션): 썸네일/미리보기 Persistent 저장 (사용자 비활성화 가능)
    *	Loader: 전용 Thread-pool + Cancelable job (스크롤 시 앞선 작업 취소)

5.3 렌더링
    *	Viewer Scene: 단일 Quad + 샘플러(Nearest/Bilinear), sRGB 고려
    *	Large Image Strategy:
    *	GPU Max texture size 초과 시 타일 분할
    *	Zoom 레벨별 LOD(Downscaled) 텍스처 우선 로딩
    *	Pan/Zoom 영역 Prefetch(화면 바깥 타일 일부)

5.4 메타데이터/색공간
    *	v1: sRGB 기준 표시(임베디드 프로파일 무시 or 단순 경고)
    *	v2: ICC Profile 파이프라인(선택), EXIF Orientation 자동회전 기본 On

5.5 구조(모듈 제안)
    *	app/ UI State + Panel/Viewer
    *	core/ 이미지 로더/캐시/타일/리사이저/포맷 프록시
    *	infra/ 파일시스템, 설정, 로깅, 플랫폼 브리지
    *	bootstrap/ 엔트리포인트, 업데이트/크래시리포트(옵션)

⸻

6) 접근성/국제화/단축키
    *	접근성: UI 대비/폰트 크기 옵션, 애니메이션 최소화
    *	국제화: i18n 구조만 준비(KO 기본, EN 리소스 템플릿)
    *	단축키(예):
    *	이동: ←/→, PageUp/Down, Home/End
    *	Zoom: +/-, Ctrl/⌘ + 휠, 더블클릭(100%↔Fit)
    *	보기: F(Fit), 1(100%), R(회전)
    *	슬라이드쇼: Space
    *	UI 토글: Tab(패널), F11(Fullscreen)

⸻

7) 품질/테스트/관측
    *	테스트 레벨
    *	Unit: 디코더/리사이저/타일러/캐시 정책
    * 데이터 처리 부분의 테스트 코드 작성

⸻

8) 보안/개인정보/라이선스
    *	로컬 처리 원칙, 외부 전송 없음
    *	사용 라이브러리 License 검토(MIT/Apache 우선)

⸻

9) 마일스톤

각 마일스톤은 Deliverables와 Acceptance Criteria를 명확히 합니다.

M0 — 프로젝트 부트스트랩
    *	Deliverables: Repo 초기화, eframe 윈도우 띄우기, 기본 메뉴바/패널 스켈레톤, 로깅/설정 파일 틀
    *	AC: 앱 실행/종료 정상, 빈 폴더 화면/더미 목록 표시, 로깅 파일 생성
    *	진행상태: [x] eframe 윈도우 스켈레톤, [x] 로깅 초기화, [x] 설정 템플릿 생성

M1 — 파일/폴더 탐색 & 기본 디코딩
    *	Deliverables: 폴더 드롭/열기, 디렉토리 스캔, 포맷 식별, 기본 디코딩(대표 포맷: PNG/JPEG)

M2 — 뷰어 코어(Zoom/Pan/Rotate)
    *	Deliverables: Zoom 단계/연속, Pan, 90° 회전, Fit/Fill/100% 모드, 더블클릭 토글

M3 — 썸네일/캐싱/프리로드
    *	Deliverables: 썸네일 비동기 로딩, LRU 메모리 캐시, 프리로드(다음/이전 N장)

M4 — 메타데이터/EXIF & 오류 처리
    *	Deliverables: EXIF Orientation/촬영일/기본 정보 표시, 손상 파일 에러 카드, 스킵/복구

M5 — 슬라이드쇼/설정/단축키
    *	Deliverables: 슬라이드쇼(간격/Loop/Shuffle), 설정 패널(캐시/프리로드/배경), 단축키 도움말

