# ezterm

<img height="96" alt="ezterm" src="https://raw.githubusercontent.com/wezterm/wezterm/main/assets/icon/wezterm-icon.svg" align="left">

**ezterm**은 [WezTerm](https://wezterm.org/)을 포크해 *마우스만으로도 정리가 끝나는* 터미널을 목표로 편의 기능을 덧붙인 GPU 가속 터미널 에뮬레이터입니다. WezTerm의 설정·Lua API·멀티플렉서는 그대로 쓸 수 있고, 그 위에 파일 매니저, 워크스페이스 사이드바, 자동 빌드·릴리즈 파이프라인이 추가되어 있습니다.

원본 WezTerm README는 [README-WEZTERM.md](README-WEZTERM.md)에 그대로 보존되어 있습니다.

<br clear="left">

## 다운로드

| 채널 | 설명 |
|---|---|
| [**Nightly**](https://github.com/ez2k/ezterm/releases/tag/nightly) | `main`에 머지될 때마다 자동 갱신. 릴리즈 설명에 빌드 번호·날짜·커밋이 표시됩니다. |
| [**릴리즈**](https://github.com/ez2k/ezterm/releases) | 마일스톤마다 태그된 버전(예: `v0.1.0`). |

Linux(`.deb`, `.tar.xz` / Ubuntu 22.04 기준 빌드), macOS(유니버설 `.zip`), Windows(`.zip`, 설치 프로그램 `.exe`)를 제공합니다. 각 파일에는 `.sha256` 체크섬이 함께 올라갑니다. macOS 빌드는 서명되어 있지 않으므로 처음 실행 시 우클릭 → 열기가 필요할 수 있습니다.

## WezTerm에 더해진 것

### 파일 매니저 — `ShowFileManager`
현재 탭 오른쪽에 **토글식 사이드바**로 열리는 파일 매니저입니다.

- 로컬 페인은 로컬 파일시스템을, **SSH 도메인 페인은 이미 맺어진 SSH 세션의 SFTP 채널**로 원격 파일시스템을 탐색합니다 (비밀번호 재입력 없음).
- 페인의 현재 디렉터리(OSC 7 또는 포그라운드 프로세스의 cwd)에서 시작합니다.
- `d` 다운로드(로컬 다운로드 폴더, 동명 파일은 `이름 (1).ext`), `u` 업로드(경로 입력), 전송 진행률 표시.
- **vi 스타일 뷰어**: 파일을 열면 `j/k`, `Ctrl-d/u`, `g/G`, `/` 검색, `n/N`, `q`로 보는 읽기 전용 페이저. 원격 파일도 SFTP로 바로 읽습니다.
- 마우스: 휠 스크롤, 클릭 선택, 선택 항목 재클릭으로 열기, **우클릭 = 뒤로**, **휠 클릭 = 앞으로** (브라우저식 히스토리).

### 워크스페이스 사이드바 — `ShowWorkspaceSidebar`
상단 탭바처럼 **창 크롬의 일부로 그려지는 왼쪽 세로 바**입니다. 페인이 아니라서 탭·워크스페이스를 바꿔도 항상 제자리에 있고, 탭바는 사이드바 옆에서 시작합니다.

- 워크스페이스 이름, 창 개수, 현재 워크스페이스 `*` 표시, 행 번호.
- 클릭으로 전환, **더블클릭으로 이름 변경**.
- 폭은 `config.workspace_sidebar_width = 20` (셀 단위).

### 새 키 액션
| 액션 | 동작 |
|---|---|
| `ShowFileManager` | 파일 매니저 사이드바 토글 |
| `ShowWorkspaceSidebar` | 워크스페이스 사이드바 토글 |
| `SwitchToWorkspaceByIndex(n)` | 이름순 n번째(0부터) 워크스페이스로 전환 — 사이드바 번호와 동일 |

모두 커맨드 팔레트(`Ctrl+Shift+P`)에도 등록되어 있습니다.

### 설정 예시
```lua
local wezterm = require 'wezterm'
local act = wezterm.action
local config = wezterm.config_builder()

config.workspace_sidebar_width = 22
config.keys = {
  { key = 'E', mods = 'CTRL|SHIFT', action = act.ShowFileManager },
  { key = 'B', mods = 'CTRL|SHIFT', action = act.ShowWorkspaceSidebar },
  { key = 'n', mods = 'CTRL|SHIFT', action = act.SwitchWorkspaceRelative(1) },
  { key = 'p', mods = 'CTRL|SHIFT', action = act.SwitchWorkspaceRelative(-1) },
}
-- ALT-1..9 로 워크스페이스 바로 전환
for i = 1, 9 do
  table.insert(config.keys, {
    key = tostring(i), mods = 'ALT', action = act.SwitchToWorkspaceByIndex(i - 1),
  })
end

return config
```

## 릴리즈 파이프라인

`.github/workflows/release.yml` 하나가 Linux/macOS/Windows를 빌드합니다.

- **`main` 푸시** → 3개 플랫폼 빌드 후 `nightly` 릴리즈의 에셋과 설명(빌드 번호·날짜·커밋)을 교체합니다.
- **버전 릴리즈**는 다음 중 하나로 만듭니다: 리포 루트의 `.release-version` 파일을 새 버전(예: `v0.2.0`)으로 바꿔 머지, Actions에서 `version` 입력과 함께 수동 실행, 또는 `v*` 태그 푸시. 릴리즈 제목에 빌드 날짜·번호가 붙고 릴리즈 노트는 자동 생성됩니다.
- 별도 시크릿 없이 `GITHUB_TOKEN`만으로 동작합니다. 업스트림의 `gen_*` 워크플로는 이 포크에서 릴리즈를 만들지 않습니다.

## 빌드하기

```sh
./get-deps                 # 시스템 의존성 (Linux/macOS)
cargo build --release -p wezterm -p wezterm-gui -p wezterm-mux-server
cargo run -p wezterm-gui   # 개발 중 바로 실행
```

포크에서 추가된 코드는 주로 `wezterm-gui/src/overlay/file_manager.rs`, `wezterm-gui/src/termwindow/render/workspace_sidebar.rs`, `config/src/keyassignment.rs`에 있습니다. 문서는 `docs/config/lua/keyassignment/` 아래에 액션별로 있습니다.

## 로드맵

진행 중인 기획: 드래그로 탭·페인 이동/분리/합치기, 우클릭 컨텍스트 메뉴(터미널·탭·사이드바·파일 매니저), 워크스페이스 닫기 확인창. 이후 후보: 세션 복원, 드롭다운(Quake) 모드, 명령 완료 알림, 키 바인딩 치트시트.

## 업스트림과 라이선스

WezTerm은 [@wez](https://github.com/wez)가 만든 프로젝트이며 ezterm은 그 위에 얹힌 포크입니다. 라이선스는 원본과 동일하게 [LICENSE.md](LICENSE.md)를 따릅니다. WezTerm 자체에 대한 문서와 지원 채널은 https://wezterm.org/ 를 참고하세요.
