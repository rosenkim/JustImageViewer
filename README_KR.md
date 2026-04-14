# Vibe Image Viewer

내가 사용하는 Windows PC와 Mac에서 같이 사용할 수 있고 내가 쉽게 기능을 확장할 수 있는 이미지 뷰어가 필요해서 만들기 시작. 거기에 Codex의 능력도 확인해보고 싶었다.

Rust,winit,wgpu,imgui-rs를 사용하여 만든 이미지 뷰어
cross platform 을 지원
GPU의 최대 Texture Size에 의존하는 한계가 있으나 크게 불편하지 않음

[ScreenShot]

Windows 11,Apple Silicon Mac 에서 테스트 및 사용 중

## 사용된 도구

- Codex and Windsurf

## 기능

- 디렉토리(폴더) 단위 이미지 탐색
- 명령행으로 단일 이미지 또는 디렉토리 열기
- 이미지 드래그 앤 드롭 열기
- 썸네일 목록 표시
- 이미지 정보 표시
- 이미지 선택 영역 및 클립 보드 복사
- 설정 저장 및 복원
- imgui 테마 지원 ('Dark', 'Light', 'Classic')
- 사용자 지정 폰트 지원

## 빌드 및 실행

Rust stable toolchain 필요

실행

```sh
git clone
cargo run
```

## 명령행 옵션

**`--reset-config`**

기존 설정 파일을 버리고, 번들된 기본 설정으로 다시 생성합니다.

**`PATH` or `File`**

이미지 파일 하나 또는 디렉토리 경로를 인자로 넘길 수 있습니다.

- 파일 경로를 주면: 단일 파일 모드로 열립니다.
- 디렉토리 경로를 주면: 디렉토리 안의 이미지를 스캔해서 목록을 보여줍니다.

## 조작법

- **Arrow Left / Arrow Up / Page Up**: 이전 이미지
- **Arrow Right / Arrow Down / Page Down**: 다음 이미지
- **Home / End** : 처음 이미지,마지막 이미지
- **Ctrl/Cmd + O**: 폴더 열기
- **Esc**: 선택 취소

또한 파일 또는 폴더를 창에 직접 드래그 앤 드롭할 수 있습니다.

## 설정 파일 위치

설정은 사용자 홈 디렉터리 아래에 저장됩니다.

- macOS / Linux: `~/.VibeImageViewer/settings.toml`
- Windows: `%USERPROFILE%/.VibeImageViewer/settings.toml`

## 참고

- GPU 텍스처 최대 크기를 넘는 큰 이미지는 표시되지 않습니다.

## 라이선스

MIT 라이선스