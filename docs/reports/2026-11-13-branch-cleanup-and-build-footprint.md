# 브랜치 정리와 빌드 산출물 절감 조사

2026-11-13, `younchan`의 `00f21e6`을 기준으로 조사했다. 로컬 브랜치 2개와 대표의 원격 작업 브랜치 1개를 정리했다. 빌드 산출물은 설정 추가보다 누적 캐시 정리가 우선이다. 현재 개발용 디버그 정보 축소와 CI 증분 캐시 비활성화가 이미 적용되어 있다.

## 브랜치 정리 결과

| 대상 | 마지막 커밋 | 처리와 보존 근거 |
|---|---|---|
| 로컬 `chan12392/younchan-lease-confirm-wt` | `5ae89ab` | `younchan`에 포함된 것을 확인하고 일반 `git branch -d`로 삭제 |
| 로컬 `younchan-state-isolation` | `55a64d5` | 미병합 시제품을 로컬 태그 `archive/younchan-state-isolation-20261113`으로 보존. 기존 워크트리를 같은 커밋의 detached HEAD로 바꾼 뒤 그 워크트리에서 일반 `git branch -d`로 삭제 |
| 원격 `origin/younchan-hotfix` | `2ed3958` | `younchan`과 `origin/test_1`의 조상임을 확인하고 삭제. 원격 조회로 삭제 완료를 다시 확인 |

핫픽스의 [PR #48](https://github.com/chan12392/project_O/pull/48)은 병합 없이 닫혔지만, 브랜치의 모든 커밋은 위 두 브랜치에 포함되어 있었다. 해당 브랜치의 열린 PR은 없었다. 고정 브랜치 9개와 다른 팀원의 작업 브랜치는 정리 대상에서 제외했다. 로컬 작업 브랜치는 `younchan` 하나가 남았다.

시제품 보존 태그가 가리키는 커밋은 `55a64d562bdd44a79ed5ae725aad6ce3ce4e7121`이다. 미병합 3개 커밋 `f72254d`, `164e0aa`, `55a64d5`를 유지하며, 시제품의 공식 CLI 호환 실패도 그대로 미해결이다. 태그는 로컬에만 있다. 저장소 루트에서 기존 워크트리에 브랜치를 복원하려면 다음 명령을 쓴다.

```powershell
git -C ../../workspaces/project_O/younchan-lease-confirm-wt switch -c younchan-state-isolation archive/younchan-state-isolation-20261113
```

워크트리 디렉터리 이동·삭제는 이번 브랜치 정리와 별개다. 기존에 차단된 파일 이동이나 ACL 변경은 재시도하지 않았다. 기존 워크트리와 사용자 감사 원본 6개를 보존했다.

## 실제 용량

2026-11-13 11:37:36 KST에 본체 저장소의 `target` 파일 길이를 합산했다. **25,769,275,245바이트, 24.000GiB, 68,733개 파일**이며 읽기 오류와 건너뛴 링크는 각각 0개였다. 논리적 파일 크기 합계이므로 하드링크 중복, 압축, 디스크 할당 단위에 따른 실제 점유량과 다를 수 있다. 다른 빌드 프로세스가 실행 중인 환경의 시점별 측정이다.

| 경로 | 논리적 크기 |
|---|---:|
| `target/debug` | 18.574GiB |
| `target/release` | 3.773GiB |
| `target/x86_64-pc-windows-msvc` | 1.379GiB |
| `target/x86_64-unknown-linux-gnu` | 0.225GiB |
| 그 밖의 파일 | 0.049GiB |

`debug` 안에서는 `incremental`이 **9.239GiB**, `deps`가 **8.704GiB**, `build`가 **0.390GiB**다. 증분 캐시 디렉터리는 698개다. 증분 캐시만 전체 `target`의 **38.5%**를 차지한다.

전체 `target`의 PDB 파일은 **5.631GiB**다. 이 값은 위 폴더 크기에 이미 포함되어 있으므로 추가로 합산하지 않는다. `debug/deps`의 `libo_core` 라이브러리·메타데이터에는 서로 다른 빌드 해시 21개가 있었다. 해시나 수정 시각만으로 특정 산출물이 더 이상 사용되지 않는다고 판정할 수는 없다.

측정 원본은 이 PC의 `C:/tmp/project-o-build-footprint-20261113.json`, 읽기 전용 조사 스크립트는 `C:/tmp/project-o-build-footprint.py`에 있다. 디렉터리 링크를 따라가지 않았고 확장자별·수정 시기별 합계가 총 바이트 수와 같은지 검증했다.

## 이미 적용된 설정

- [Cargo.toml](../../Cargo.toml): 개발 빌드는 `debug = "line-tables-only"`, 외부 의존 패키지는 `debug = false`다. 2026-11-04의 `a3770ed`에서 적용했다. 테스트는 개발 프로필을 상속한다. `debug = 1`로 바꾸는 것은 현재보다 정보를 줄이는 조치가 아니다. [Cargo 프로필 문서](https://doc.rust-lang.org/cargo/reference/profiles.html#debug)
- [guard.yml](../../.github/workflows/guard.yml): Linux와 Windows 검사 모두 `CARGO_INCREMENTAL: "0"`이다. CI에 같은 설정을 더할 필요가 없다.
- 릴리스에는 이미 `lto = "thin"`, `codegen-units = 1`, `strip = true`가 있다. 일상 검사에는 기존 규칙대로 개발 프로필을 쓴다.
- 현재 환경에서는 `CARGO_INCREMENTAL`과 별도 target 디렉터리 설정을 찾지 못했고, Cargo 메타데이터의 출력 경로는 저장소의 `target`이었다.

## 절감 우선순위와 비용

| 순서 | 조치 | 기대 효과 | 비용·제약 |
|---|---|---|---|
| 1 | 사용이 끝난 증분 캐시를 정리하고, 일회성 로컬 검증에서는 `CARGO_INCREMENTAL=0` 사용 | 측정 당시 증분 캐시 전체의 논리적 상한은 9.239GiB. 일회성 검증의 새 증분 캐시 생성을 억제 | 기존 캐시는 설정만 바꿔도 사라지지 않는다. 자주 수정하는 개발 빌드는 증분 캐시를 유지하는 편이 재컴파일에 유리 |
| 2 | 종료한 패키지·프로필·타깃의 산출물만 정리 | 플랫폼별 별도 출력 1.604GiB, release 3.773GiB 중 실제 사용이 끝난 부분을 회수할 수 있음 | 현재 실행 파일·설치 산출물·진행 중인 빌드부터 확인해야 한다. 이후 해당 빌드는 다시 생성 |
| 3 | 개발용 디버그 정보를 완전히 끄는 방안은 별도 비교 후 판단 | PDB 등 추가 감소 가능, 절감량은 미측정 | 오류 위치와 디버깅 정보가 줄어든다. PDB 5.631GiB 전부가 없어지는 것으로 계산하면 안 됨 |

증분 컴파일은 재컴파일을 빠르게 하려고 추가 정보를 디스크에 저장한다. `CARGO_INCREMENTAL`은 이를 덮어쓰는 공식 환경변수다. 따라서 반복 개발 전체에 전역으로 끄기보다, 반복 사용하지 않을 검증 프로세스에 한정하는 것이 우선 권고다. 실제 시간 차이는 이 조사에서 벤치마크하지 않았다. [Cargo 증분 컴파일 문서](https://doc.rust-lang.org/cargo/reference/profiles.html#incremental)

Cargo의 자동 캐시 정리는 현재 다운로드한 전역 레지스트리·소스 캐시를 대상으로 하며, 빌드 산출물 추적은 지원하지 않는다. 자동 정리 주기만 설정해도 이 `target`이 줄어든다고 기대할 수 없다. [Cargo 캐시 문서](https://doc.rust-lang.org/cargo/reference/config.html#cache)

실행 중인 빌드가 끝난 뒤, 정리 후보의 범위를 확인할 수 있는 읽기 전용 명령은 다음과 같다. 이번에는 아래 명령이나 실제 정리를 실행하지 않았다.

```powershell
cargo clean -p o-core --profile dev --dry-run
cargo clean --target x86_64-unknown-linux-gnu --profile dev --dry-run
cargo clean --profile release --dry-run
```

`--dry-run`은 삭제 예정 요약을 보여 주며 `--verbose`를 더하면 파일 목록을 확인할 수 있다. 패키지·프로필·타깃을 지정해 범위를 제한할 수 있다. 설치된 Cargo의 도움말에서도 해당 옵션을 확인했다. [cargo clean 문서](https://doc.rust-lang.org/cargo/commands/cargo-clean.html)

동일한 디스크의 `Desktop/delete`로 이동하거나 target 경로만 옮기는 것은 디스크 여유 공간을 늘리지 않는다. 새 캐시 도구 설치나 모든 워크트리의 출력 경로 공유는 이번 권고에 포함하지 않았다.

## 검증 범위

- 브랜치 포함 관계, 시제품의 깨끗한 작업 트리와 태그의 정확한 커밋, 삭제 후 로컬·원격 참조를 확인했다.
- 조사 환경은 Cargo 1.97.1 / rustc 1.97.1 / `x86_64-pc-windows-msvc`다. 최신 공식 문서는 Cargo 1.100.0에서 중간 산출물 구조가 변경되었다고 명시하므로, 이 PC의 경로 판단은 실제 디렉터리와 Cargo 메타데이터를 기준으로 했다. [Cargo 빌드 캐시 문서](https://doc.rust-lang.org/cargo/reference/build-cache.html)
- 빌드 설정 변경·캐시 삭제·전체 재빌드는 하지 않았다. 위 절감량은 논리적 크기 기준 후보이며, 실제 회수량과 재빌드 시간은 검증하지 않았다.
