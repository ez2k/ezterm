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
- `m` 또는 **Shift/Ctrl+우클릭**: 열기/보기, 다운로드, 업로드, 상위, 뒤로/앞으로, 새로고침, 닫기 액션 메뉴.

### 워크스페이스 사이드바 — `ShowWorkspaceSidebar`
상단 탭바처럼 **창 크롬의 일부로 그려지는 왼쪽 세로 바**입니다. 페인이 아니라서 탭·워크스페이스를 바꿔도 항상 제자리에 있고, 탭바는 사이드바 옆에서 시작합니다.

- 워크스페이스 이름, 창 개수, 현재 워크스페이스 `*` 표시, 행 번호.
- 클릭으로 전환, **더블클릭으로 이름 변경**.
- **우클릭 메뉴**: 전환, 이름 변경, 워크스페이스에 새 창, 워크스페이스 닫기, 새 워크스페이스, 사이드바 숨기기.
- **워크스페이스 닫기는 항상 확인창**을 거칩니다. 닫힐 창/탭/페인 수와 실행 중인 프로그램을 보여주고, 마지막 워크스페이스면 종료됨을 경고합니다.
- 탭을 사이드바 행에 **드롭하면 그 워크스페이스로 이동**합니다.
- 폭은 `config.workspace_sidebar_width = 20` (셀 단위).

### 우클릭 컨텍스트 메뉴 — `ShowContextMenu`
- **터미널 영역 우클릭** (앱이 마우스를 잡고 있지 않을 때): 링크 열기, 복사/붙여넣기, 복사 모드, 분할(오른쪽/아래), 줌, 페인 닫기, 새 탭, 작업 디렉터리 복사, 파일 매니저, 커맨드 팔레트.
- **탭 우클릭**: 활성화, 새 탭, 복제, 이름 변경, 왼쪽/오른쪽으로 이동, 새 창으로 분리, 닫기, 다른 탭 닫기, 오른쪽 탭 닫기.
- 키보드로도 조작 가능(`↑/↓`, `j/k`, `Enter`, `Esc`). `context-menu` Lua 이벤트에서 `false`를 반환하면 기본 메뉴를 막을 수 있고, 기본 우클릭 바인딩은 `DisableDefaultAssignment`로 끌 수 있습니다.

### 페인 드래그
- `Ctrl+Alt`(`config.pane_drag_modifiers`) + 왼쪽 버튼으로 페인을 끌 수 있습니다.
- 다른 페인의 **가장자리에 드롭하면 그쪽으로 분할**, **가운데에 드롭하면 자리 바꾸기**.
- 탭에 드롭하면 그 탭으로 이동, 탭바 빈 곳/＋ 버튼에 드롭하면 **새 탭**, 사이드바 행에 드롭하면 그 워크스페이스, 창 밖에 드롭하면 **새 창**.
- 드롭 영역이 반투명으로 표시되며 `Esc`로 취소. 자세한 내용은 `docs/config/pane-drag.md`.

### 탭 드래그
- 탭바 안에서 드래그하여 **순서 변경** (삽입 위치 표시).
- **창 밖에 드롭**하면 포인터 위치에 **새 창으로 분리**.
- **워크스페이스 사이드바 행에 드롭**하면 그 워크스페이스로 이동.
- 드래그 중 `Esc`로 취소. 자세한 내용은 `docs/config/tab-drag.md`.

### 새 키 액션
| 액션 | 동작 |
|---|---|
| `ShowFileManager` | 파일 매니저 사이드바 토글 |
| `ShowWorkspaceSidebar` | 워크스페이스 사이드바 토글 |
| `SwitchToWorkspaceByIndex(n)` | 이름순 n번째(0부터) 워크스페이스로 전환 — 사이드바 번호와 동일 |
| `ShowContextMenu` | 마우스 위치에 컨텍스트 메뉴 (기본: 우클릭) |
| `DuplicateTab` | 같은 도메인·디렉터리로 탭 복제 |
| `RenameTab` | 탭 제목 변경 프롬프트 |
| `MoveTabToNewWindow` | 현재 탭을 새 창으로 분리 |
| `CloseOtherTabs { confirm }` | 현재 탭 외 모두 닫기 |
| `CloseTabsToTheRight { confirm }` | 오른쪽 탭 모두 닫기 |
| `CopyCurrentWorkingDir` | 현재 페인의 작업 디렉터리를 클립보드로 |

설정: `config.pane_drag_modifiers = "CTRL|ALT"` (페인 드래그 시작 조합키).

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

완료: 우클릭 컨텍스트 메뉴(터미널·탭·사이드바·파일 매니저), 워크스페이스 닫기 확인창, 탭 드래그(순서 변경·새 창 분리·워크스페이스 이동). 완료(계속): 페인 드래그(분할·교체·탭/워크스페이스/새 창 이동). 진행 중: 탭↔페인 합치기, 파일 매니저 삭제/이름 변경. 이후 후보: 세션 복원, 드롭다운(Quake) 모드, 명령 완료 알림, 키 바인딩 치트시트.

## 업스트림과 라이선스

WezTerm은 [@wez](https://github.com/wez)가 만든 프로젝트이며 ezterm은 그 위에 얹힌 포크입니다. 라이선스는 원본과 동일하게 [LICENSE.md](LICENSE.md)를 따릅니다. WezTerm 자체에 대한 문서와 지원 채널은 https://wezterm.org/ 를 참고하세요.
