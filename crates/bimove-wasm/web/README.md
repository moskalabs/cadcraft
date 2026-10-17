# 손으로 돌려보는 하네스

`bimove-wasm` 을 브라우저에서 띄워 보는 최소 페이지. Bimove 본체에 꽂기 전에
"정말 되는가" 를 눈으로 확인하는 용도다.

## 쓰기

```sh
# 1. wasm 빌드 + JS 바인딩
cargo build --release -p bimove-wasm --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir crates/bimove-wasm/web/pkg \
  target/wasm32-unknown-unknown/release/bimove_wasm.wasm

# 2. DXF 와 한글 폰트를 같은 폴더에 둔다
#    - 아무 .dxf (examples/apartment.dxf 가 작아서 좋다)
#    - NotoSansKR.ttf (OFL. 없으면 ?nofont=1 로 끈다)

# 3. 띄운다
cd crates/bimove-wasm/web && python -m http.server 8731
# http://localhost:8731/raster.html?f=apartment.dxf
```

`?f=` 로 파일, `?nofont=1` 로 폰트 등록 생략.

## 알아둘 것

- **wasm 에는 시스템 폰트가 없다.** `registerFont` 로 넣지 않으면 글자가 전부
  □ 로 나온다. 폰트 문제로 보이는 것이 사실은 DXF 인코딩 문제인 경우도 있으니
  (`$DWGCODEPAGE` 가 거짓말하는 변환기가 있다) 둘을 구분해서 보라.
- `window.__cam()` / `__worldAt(sx,sy)` / `__panBench()` / `__bench()` 로
  카메라와 성능을 재볼 수 있다.
- 합성 포인터·휠 이벤트의 `clientX`/`clientY` 는 **정수로 잘린다**. 소수 좌표로
  테스트하면 있지도 않은 어긋남을 보게 된다 (실제로 한 번 속았다).
