# glib 0.18.5 보안 백포트

`glib-0.18.5/`는 crates.io 원본 121개 파일에 공식 수정 **두 줄만** 적용한 소스다. 버전은 0.18.5를 유지한다. 루트 `[patch.crates-io]`가 GTK3/WebKitGTK 의존 경로 전체에 적용한다. Windows에서는 이 의존 경로가 선택되지 않는다.

- 원본: https://static.crates.io/crates/glib/glib-0.18.5.crate
- 원본 SHA-256: `233daaf6e83ae6a12a52055f568f9d7cf4671dabb78ff9560ab6da230ce00ee5`
- 공식 수정: https://github.com/gtk-rs/gtk-rs-core/commit/b5a4071e439bef2b5eea76c3aa25e5ae84839e34 ([PR #1343](https://github.com/gtk-rs/gtk-rs-core/pull/1343))
- 경보: [RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html), [GHSA-wrw7-89jp-8q8g](https://github.com/advisories/GHSA-wrw7-89jp-8q8g)
- 적용 차이: [glib-0.18.5.patch](glib-0.18.5.patch). C out-pointer를 `let mut p`, `&mut p`로 고친다.
- 패치 후 `src/variant_iter.rs` SHA-256: `a0f5ee8acb8faa089bcdfbc9a57372609fce7654026ccef7d9a224d05a654ccc`
- 패치 후 전체 트리 SHA-256: `2f7ef2019fd0cb8cb2b95f3a3a4d08b63c4ea546362eae332643a6a6de0e67bf`
- 원본 [MIT LICENSE](glib-0.18.5/LICENSE)와 [COPYRIGHT](glib-0.18.5/COPYRIGHT)를 그대로 포함한다.

트리 해시는 상대 POSIX 경로를 대소문자 구분 정렬하여 각 파일의 `경로 + NUL + SHA256(원본 바이트) + LF`를 연결한 UTF-8 바이트의 SHA-256이다. `.gitattributes`가 자동 줄바꿈 변환을 막는다. 추가·누락·변경·심볼릭 링크가 있으면 `scripts/check-dependencies.py`가 실패한다. 해시 기대값은 검사기에 고정되어 있으며 변경 소스의 자기 신고를 신뢰하지 않는다.

0.18 마지막 공식 배포는 0.18.5다. 2026-11-12 확인 당시 최신 안정 Tauri 2.12.5는 gtk 0.18을, WebKitGTK 2.0.2는 glib/gtk ^0.18을 요구한다. 새 gtk 0.19.0은 glib ^0.22를 쓰므로 현재 Tauri/WebKitGTK 조합에 대입할 수 없다. 공식 0.18 브랜치 `42b9caf98e03ded086362d9653ca58fe94dc8658`에도 이 수정은 없다.

유지 비용: 소스 약 1.72 MB를 보관하고, 새 glib 경보를 기존 **0.18.5 버전으로 계속 조회**한다. 다른 취약점·unsound 경보는 실패한다. 새 백포트나 의존 경로 교체 시 소스·해시·회귀 테스트를 함께 검토해야 한다. 기준 브랜치의 검사기가 새 소스를 거부하므로 검사 정책 변경을 먼저 검토·반영한다. 호환 공식 버전으로 전환할 수 있게 되면 이 패치·해시 예외·전용 fixture를 제거한다.

Linux 회귀 검증은 `tests/glib-variant/`를 참조한다. GLib 개발 라이브러리가 있는 Linux에서:

```sh
cargo test --locked --manifest-path tests/glib-variant/Cargo.toml
CARGO_PROFILE_TEST_OPT_LEVEL=2 cargo test --locked --manifest-path tests/glib-variant/Cargo.toml
```

최적화 빌드도 dev/test 프로필을 쓰며 release/LTO는 사용하지 않는다. 전용 fixture는 일반 문자열·빈 문자열·Unicode, 양방향/교차 순회, nth/nth_back/last, 빈 배열·경계 초과·잘못된 Variant 타입을 검증한다.
