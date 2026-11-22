# 코드 서명

릴리스 워크플로(`.github/workflows/release.yml`)는 GitHub 저장소의 **Settings → Secrets and variables → Actions**에 비밀값이 있으면 설치 파일에 서명하고, 없으면 지금처럼 서명 없이 빌드합니다. 워크플로 파일은 고칠 필요가 없습니다.

## 먼저 알아 둘 것 (2026년 기준)

- **SmartScreen 경고는 서명만으로 바로 사라지지 않습니다.** 2024년 8월부터 EV 인증서도 OV와 똑같이 취급되어, 서명된 파일도 다운로드가 쌓여 평판이 생길 때까지 「Windows의 PC 보호」 창이 뜰 수 있습니다. 서명하면 경고 창에 「알 수 없는 게시자」 대신 게시자 이름이 표시되고, 같은 인증서로 서명한 다음 버전부터는 평판이 이어집니다.
- **EV 인증서는 SmartScreen 때문이라면 살 이유가 없습니다.** OV 인증서로 충분합니다.
- **Azure Artifact Signing(구 Trusted Signing, 월 $9.99)은 한국 개인 개발자가 쓸 수 없습니다.** 개인은 미국·캐나다, 조직은 미국·캐나다·EU·영국만 가능합니다.
- 2023년 6월부터 공인 코드 서명 인증서의 개인 키는 하드웨어 토큰이나 클라우드 HSM에 보관해야 합니다. 그래서 새로 사는 인증서는 대부분 PFX 파일로 받을 수 없고, 클라우드 서명(방법 B)을 써야 CI에서 서명할 수 있습니다.
- 2026년 3월부터 코드 서명 인증서의 최대 유효 기간은 458일입니다. 해마다 갱신한다고 생각하면 됩니다.
- **Microsoft Store로 배포하면 경고가 없습니다.** Store가 Microsoft 인증서로 서명하므로, 일반 사용자 배포가 목적이라면 가장 확실한 방법입니다.

## Windows

### 방법 A — PFX 인증서 파일

인증서를 PFX 파일로 가지고 있을 때 씁니다(사내 인증서, 예전에 발급받은 인증서 등).

| 비밀값 | 값 |
| --- | --- |
| `WINDOWS_CERTIFICATE` | PFX 파일을 base64로 바꾼 문자열. PowerShell: `[Convert]::ToBase64String([IO.File]::ReadAllBytes("cert.pfx")) \| Set-Clipboard` |
| `WINDOWS_CERTIFICATE_PASSWORD` | PFX 암호 |

워크플로가 인증서를 빌드 머신의 인증서 저장소에 넣고, 지문(thumbprint)으로 Tauri에 넘깁니다. SHA-256과 DigiCert 타임스탬프 서버를 씁니다.

### 방법 B — Azure Key Vault (클라우드 HSM)

OV 인증서를 Azure Key Vault에 발급받거나 가져온 경우입니다. Tauri 공식 문서가 안내하는 방식으로, [relic](https://github.com/sassoftware/relic)이 Key Vault의 키로 서명합니다. 개인 키는 빌드 머신에 오지 않습니다.

1. Key Vault를 만들고(Premium 등급이면 HSM 보관) 인증서를 넣습니다. DigiCert, GlobalSign 등은 Key Vault로 바로 발급할 수 있습니다.
2. Microsoft Entra ID에 앱(서비스 주체)을 등록하고 클라이언트 암호를 만듭니다.
3. Key Vault에서 이 앱에 「Key Vault Crypto User」와 「Key Vault Certificate User」 역할을 줍니다.

| 비밀값 | 값 |
| --- | --- |
| `AZURE_KEY_VAULT_CERT_URL` | `https://<볼트 이름>.vault.azure.net/certificates/<인증서 이름>` |
| `AZURE_TENANT_ID` | 디렉터리(테넌트) ID |
| `AZURE_CLIENT_ID` | 앱(클라이언트) ID |
| `AZURE_CLIENT_SECRET` | 클라이언트 암호 |

두 방법의 비밀값이 모두 있으면 방법 A를 씁니다.

## macOS

Apple Developer Program(연 $99) 가입이 필요합니다. 「Developer ID Application」 인증서로 서명하고, Apple ID를 넣으면 공증(notarization)까지 합니다. 공증된 앱은 Gatekeeper 경고 없이 열립니다.

| 비밀값 | 값 |
| --- | --- |
| `APPLE_CERTIFICATE` | Developer ID Application 인증서를 .p12로 내보내 base64로 바꾼 문자열 (`base64 -i cert.p12 \| pbcopy`) |
| `APPLE_CERTIFICATE_PASSWORD` | .p12 암호 |
| `APPLE_SIGNING_IDENTITY` | 예: `Developer ID Application: 홍길동 (ABCDE12345)` |
| `APPLE_ID` | 공증에 쓸 Apple ID 이메일 |
| `APPLE_PASSWORD` | appleid.apple.com에서 만든 앱 암호 |
| `APPLE_TEAM_ID` | 팀 ID (10자리) |

`APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`를 비워 두면 서명만 하고 공증은 하지 않습니다.

## 확인

태그를 푸시한 뒤 릴리스 워크플로 로그의 「코드 서명 준비」 단계에 「…로 서명합니다」가 나오면 서명된 것입니다. 받은 설치 파일은 Windows에서 파일 속성 → **디지털 서명** 탭, macOS에서는 `codesign -dv --verbose=4 /Applications/CellX.app`과 `spctl -a -vv /Applications/CellX.app`으로 확인합니다.
