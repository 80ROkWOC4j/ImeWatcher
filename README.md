# ImeWatcher
Detect Windows IME language change.  
윈도우 IME의 언어 변경을 감지합니다.

# 할 수 있는 것
- 날개셋 입력기 사용 불가능한 윈도우 환경(vdi 원격 등)에서 윈도우 내장 두벌식(혹은 내장 세벌식) 레이아웃 쓰면서 커스텀 자판도 쓰고 싶은 경우  
- 한/영 상태에 따라 키보드 라이트 등 바꾸고 싶은 경우  
- 한글 전용 레이어 사용하면서 적절히 auto switch

# 설정 파일 (config.toml)

ImeWatcher는 실행 파일과 같은 디렉터리에 `config.toml`을 저장합니다.

## 기능
- **키보드별 설정**: 여러 키보드를 연결했을 때, 각 키보드마다 다른 Language→Layer 매핑 저장
- **마지막 키보드 복원**: 앱 재시작 시 마지막으로 사용한 키보드 자동 선택
- **확장 가능한 스키마**: 향후 VIA 설정(백라이트 등) 추가 예정

## 설정 예시

```toml
version = 1

# 마지막으로 사용한 키보드 (자동으로 업데이트됨)
last_keyboard_id = "1234:5678:ff60:sn:ABC123"

[keyboards]

# 키보드별 설정 (키보드 ID는 자동으로 생성됨)
[keyboards."1234:5678:ff60:sn:ABC123"]
label = "My Keyboard (1234:5678)"
vid = 4660
pid = 22136
usage_page = 65376

# Language → Layer 매핑 (16진수 언어 ID = 레이어 번호)
[keyboards."1234:5678:ff60:sn:ABC123".lang_layer]
"0x0409" = 0   # English → Layer 0
"0x0412" = 1   # Korean → Layer 1
```

## 키보드 ID 생성 규칙
- **시리얼 번호가 있는 경우**: `{vid:04x}:{pid:04x}:{usage_page:04x}:sn:{serial}`
- **시리얼 번호가 없는 경우**: `{vid:04x}:{pid:04x}:{usage_page:04x}:path:{fnv_hash}`
  - HID 경로의 FNV-1a 해시값을 사용하여 동일한 VID/PID를 가진 여러 키보드를 구분

## VIA 설정 (비영속)

VIA 호환 키보드일 경우, IME 언어 변화에 따라 조명/오디오 설정을 **비영속(EEPROM write 없음)** 으로 적용할 수 있습니다.

지원 범위 (v1):
- 조명: backlight / rgblight / rgb_matrix / led_matrix (지원되는 채널을 자동 선택)
- 오디오: enabled / clicky enabled

주의:
- 키맵/매크로/EEPROM reset/bootloader 등 **영속 설정 변경은 의도적으로 지원하지 않습니다.**

예시:

```toml
[keyboards."1234:5678:ff60:sn:ABC123".via.lang."0x0412".lighting]
brightness = 64
effect = 1
speed = 128
color_h = 10
color_s = 200

[keyboards."1234:5678:ff60:sn:ABC123".via.lang."0x0412".audio]
enabled = true
clicky = false
```

# QMK 펌웨어 연동 (IME → 레이어 스위치)

ImeWatcher는 윈도우 IME 언어(한/영 등) 변화에 맞춰 키보드의 QMK 레이어를 자동으로 전환할 수 있습니다.  
레이어 변환을 위해선 펌웨어에 기능 추가가 필요합니다.

### 제공 파일

아래 두 파일을 **자신의 keymap 폴더**로 복사하세요.

- `firmware/qmk/imewatcher_rawhid.c`
- `firmware/qmk/imewatcher_rawhid.h`

예시 경로:

`qmk_firmware/keyboards/<keyboard>/keymaps/<keymap>/`

### 기능 활성화

keymap의 `rules.mk`에 다음을 추가하세요.

```make
VIA_ENABLE = yes
RAW_ENABLE = yes

SRC += imewatcher_rawhid.c
```

> - 이미 keymap이 `via_command_kb()` 또는 `raw_hid_receive_kb()`를 구현하고 있다면 심볼 충돌을 피하기 위해 로직을 병합해야 합니다. 
> - `imewatcher_rawhid.c`의 `imewatcher_handle_rawhid_packet(...)`를 기존 핸들러에서 호출하는 형태로 합칠 수 있습니다.

### 빌드 / 플래시 (qmk_firmware)

```sh
qmk compile -kb <keyboard> -km <keymap>
qmk flash   -kb <keyboard> -km <keymap>
```

### Vial 빌드 (vial-qmk)

Vial은 훅이 `raw_hid_receive_kb`로 다르지만, 동일 모듈로 지원합니다.
`vial-qmk` 트리로 빌드하면서 아래 설정을 사용하세요.

```make
VIAL_ENABLE = yes
RAW_ENABLE  = yes

SRC += imewatcher_rawhid.c
```

### 프로토콜

ImeWatcher는 레이어 변경을 위해 32바이트 Raw HID 패킷을 보냅니다.

- `data[0] = 0x21`
- `data[1..4] = "IMEW"`
- `data[5] = 0x01` (default layer set)
- `data[6] = layer_index`
- `data[7]`는 응답 status로 사용

펌웨어 동작:

- `default_layer_set(1 << layer_index)` (비영속 전환; EEPROM write 없음)

# License
ImeWatcher is licensed under the GNU General Public License v3.0 only (GPL-3.0-only). See `LICENSE`.

This project uses `qmk-via-api` (GPL-3.0-only), which is based in parts on `the-via/app` (GPL-3.0).
