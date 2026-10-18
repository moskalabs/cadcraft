# 빌드 산출물 (커밋된다)

`wasm-bindgen --target web` 이 만든 것. **손으로 고치지 말 것** — 다음 빌드에
덮어쓰인다.

여기 커밋하는 이유는 **Bimove 의 배포 때문**이다. Vercel 빌드에는 Rust 도
이 포크도 없어서 거기서 만들 수가 없다. Bimove 의 prebuild 가 이 파일들을
받아간다 (scripts/build-cadcraft-wasm.mjs).

Bimove 레포에 두지 않는 이유는 12MB 짜리가 그쪽 히스토리에 매번 쌓이기
때문이다. 산출물은 제 소스 옆에 두는 게 맞기도 하다.

엔진을 고쳤으면 **여기를 다시 만들어 같이 커밋**해야 배포에 반영된다:

```sh
cargo build --release -p bimove-wasm --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir crates/bimove-wasm/pkg \
  target/wasm32-unknown-unknown/release/bimove_wasm.wasm
```
