# ImeWatcher
Detect Windows IME langauge change.  
윈도우 IME의 언어 변경을 감지합니다.

# 목표
- [x] 창별로 윈도우 IME 언어 감지
- [x] QMK via 장치 연동
- [ ] IME 언어 변경에 따른 각종 작동 수행 설정을 gui로 제공
  - [ ] 언어 변경에 따른 레이어 스위치 등을 제공하려면 gui 말고 json 스위칭으로 해야 할 수도 
- [ ] via를 사용하지 않는 펌웨어를 위한 펌웨어 코드 제공

# 현재 작동하는 기능
IME 한글, 영어 상태 감지해서 qmk-via 키보드의 백라이트 키고 끄기
