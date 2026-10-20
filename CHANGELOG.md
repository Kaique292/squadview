# Changelog

## [0.3.1](https://github.com/Kaique292/squadview/compare/v0.3.0...v0.3.1) (2026-10-20)


### 🐛 Correções

* **app:** não cai quando a atualização abre o app duas vezes ([d2047ff](https://github.com/Kaique292/squadview/commit/d2047ff3b79117510296f25932d677047cc246c0))
* **ci:** teste de fumaça espera só o instalador, não o app que ele abre ([fb3d932](https://github.com/Kaique292/squadview/commit/fb3d932ef9e2f0bdf8785777954848a0ed43f8fe))


### 📦 Build

* **deps:** bump sha2 from 0.10.9 to 0.11.1 ([#6](https://github.com/Kaique292/squadview/issues/6)) ([2eed7da](https://github.com/Kaique292/squadview/commit/2eed7da2722177704c993513c5bf251a7b316065))

## [0.3.0](https://github.com/Kaique292/squadview/compare/v0.2.0...v0.3.0) (2026-10-20)


### ✨ Novidades

* **app:** instala e atualiza em silêncio, como o Discord ([ad25622](https://github.com/Kaique292/squadview/commit/ad25622d5d2d950852cdbeb5c3af7c92afb95cd1))
* **app:** mostra a foto do Discord de quem está na sala ([af3567a](https://github.com/Kaique292/squadview/commit/af3567a4be22c4911081baf0f6f839b273af7218))
* **app:** página de amigos, presença e pedidos para entrar na sala ([f525593](https://github.com/Kaique292/squadview/commit/f525593d2fd55bc95cb5fbd7eb4ce835cec2b9b3))
* **net:** cliente do registro de amigos e sessão ao entrar na sala ([66763b5](https://github.com/Kaique292/squadview/commit/66763b5f96017682640e115720f2c3656657814a))
* **net:** liga a voz entre todos da sala ([ebc5f5d](https://github.com/Kaique292/squadview/commit/ebc5f5d355ef56c3cc8be57b748ac75beda8660a))
* **signal:** amigos, presença e pedido para entrar na sala ([e4f3268](https://github.com/Kaique292/squadview/commit/e4f32684530bb1f766d53a03246a5104f84a7d50))
* **signal:** avisa a sala quando alguém silencia o microfone ou ensurdece ([dd531a4](https://github.com/Kaique292/squadview/commit/dd531a46445e39bd996b676fab886a757d426065))
* **signal:** leva a conta do Discord de cada membro da sala ([0375b09](https://github.com/Kaique292/squadview/commit/0375b09c128c4b1c0725851e9b81691609b84ddc))
* **ui:** canal de voz com fundo preto e controles que aparecem com o mouse ([2754a64](https://github.com/Kaique292/squadview/commit/2754a641eb8f8e78fd085de64069e506e9248fc3))
* **ui:** chat de voz na sala com a grade e os controles do Discord ([5879f2d](https://github.com/Kaique292/squadview/commit/5879f2d2105245d9dfe6f13203ba0e9f1745c228))
* **ui:** configurações em seções como as do Discord e listas suspensas próprias ([35335de](https://github.com/Kaique292/squadview/commit/35335de87aab2c209ca8123209c6fd325247c471))


### 🐛 Correções

* **app:** garante que a janela aparece e registra erros fatais na abertura ([e06a102](https://github.com/Kaique292/squadview/commit/e06a1024a469fde23e0bcc74577f664d2b162403))
* **ui:** configurações abrem sem o contorno de foco no primeiro botão ([33f500b](https://github.com/Kaique292/squadview/commit/33f500bdf1d650d9288be59c30faecfe47e038e9))
* **ui:** diminui os botões da barra da voz para o tamanho do Discord ([b2264cd](https://github.com/Kaique292/squadview/commit/b2264cdac08c0c762e489662d1fec9e43fd05eba))
* **ui:** seta do botão de voz fica vermelha junto com ele ([a85fcb3](https://github.com/Kaique292/squadview/commit/a85fcb379ce1646e47069c4bd5d84a6aeb1a082d))
* **ui:** volume de entrada e de saída na largura toda do menu ([b508be3](https://github.com/Kaique292/squadview/commit/b508be3588a1202f4914374bfccb2071470d8ccb))


### 📝 Documentação

* README e site com as telas e os recursos da 0.3.0 ([8e1f3fb](https://github.com/Kaique292/squadview/commit/8e1f3fbc5f2e942b8e7f43bad31757ae9f0c3743))

## [0.2.0](https://github.com/Kaique292/squadview/compare/v0.1.0...v0.2.0) (2026-10-19)


### ✨ Novidades

* **app:** mostra a atualização como no Discord, com o ícone verde na barra de título ([9dc88d5](https://github.com/Kaique292/squadview/commit/9dc88d5ae41c51dcb59bdd8d21e8ecf2434f48ba))
* **media:** lista microfones e saídas e captura o microfone ([057f183](https://github.com/Kaique292/squadview/commit/057f1833ab238d4a737065bf494f1221648d5d8e))
* **media:** tira a foto do último quadro ao parar o vídeo ([c777ed0](https://github.com/Kaique292/squadview/commit/c777ed01e5c30a9f5d21d2f4acf9325873874c10))
* **net:** entra com o nome da sala, recebe renomeações e troca o apelido ([8582c58](https://github.com/Kaique292/squadview/commit/8582c5884cf1c35197dcc94544e7577ce8546788))
* **signal:** guarda o nome da sala e deixa trocar o apelido ([077e5a3](https://github.com/Kaique292/squadview/commit/077e5a3a186220d623bc740547f9d8d964ff02ef))
* **ui:** adiciona o controle de volume de quem você assiste, como no Discord ([2de8cc9](https://github.com/Kaique292/squadview/commit/2de8cc9be848c4d7b4af5928c40f1e35f5a3daaa))
* **ui:** deixa escolher o nome da sala e trocar o apelido pelo botão direito ([e8c3d2a](https://github.com/Kaique292/squadview/commit/e8c3d2aae11a77f46e4638aac1493b73c2e98f5a))
* **ui:** desfaz o vídeo em poeira ao parar, como o Safari do iOS ([35b42be](https://github.com/Kaique292/squadview/commit/35b42beeae5ccd802b3ecdd89e3d9b524910bf69))


### 🐛 Correções

* **app:** abre a janela no centro da tela ([41295d7](https://github.com/Kaique292/squadview/commit/41295d7f3b0553a4c112369ccb4c9974d9ad12e5))
* **app:** baixa o instalador direto pelo botão do site e do convite ([87ea442](https://github.com/Kaique292/squadview/commit/87ea44266a950bcb20362c057c9ce18ecdf93fc7))
* **app:** mostra a splash só depois de desenhada ([7d8efa5](https://github.com/Kaique292/squadview/commit/7d8efa520bfd6270bce0a5df4cf83ab49d0212a9))
* **net:** mantém a ligação com a sala viva e reconecta sozinho quando cai ([d3524b8](https://github.com/Kaique292/squadview/commit/d3524b826cfbd26cb9199ee10defbacff7e0126f))
* **net:** mantém a porta pública viva e explica quando o NAT é simétrico ([1cae7c4](https://github.com/Kaique292/squadview/commit/1cae7c44aed3dd5225169f6640d7a331c2199557))
* **ui:** tira a piscada entre o vídeo e a animação de saída ([9a8c47e](https://github.com/Kaique292/squadview/commit/9a8c47eefdd174aec5730e9c295c9e346c471c6f))


### 📝 Documentação

* atualiza README e roadmap para a 0.1.0, com telas reais ([b6d37ca](https://github.com/Kaique292/squadview/commit/b6d37ca7c6e89ac52d70f96112a463a3959a60b0))
* cria o site do projeto com prévia para links no Discord ([9e21ef3](https://github.com/Kaique292/squadview/commit/9e21ef33bc3864dffb55f44bd644ce0b9e00fb57))
* lista a ADR 0010 no índice ([4349891](https://github.com/Kaique292/squadview/commit/4349891e3cded98b8dae45c38dfddad7fa9a6211))


### 📦 Build

* **deps:** bump getrandom from 0.3.4 to 0.4.3 ([#4](https://github.com/Kaique292/squadview/issues/4)) ([4650974](https://github.com/Kaique292/squadview/commit/46509741d796cd691a9b13abbe901845ce408fcb))

## 0.1.0 (2026-10-19)


### ✨ Novidades

* **app:** adiciona atualização automática assinada ([476e3dd](https://github.com/Kaique292/squadview/commit/476e3dd21691246369f7f8d635d7cd65e437f2f3))
* **app:** convida amigos por link e mostra a sala no status do Discord ([61bff3e](https://github.com/Kaique292/squadview/commit/61bff3eee00c66030917e34df369ec8ac9106c13))
* **app:** cria o app desktop com o layout base da interface ([9f99dd7](https://github.com/Kaique292/squadview/commit/9f99dd730349b7eed937daba1dcaf8278db00686))
* **app:** entra com a conta do Discord ([6c29063](https://github.com/Kaique292/squadview/commit/6c290634cdddd2961624c0b89860b455b402e7fa))
* **app:** liga e desliga o registro de desempenho e copia os arquivos para mandar ([080edd0](https://github.com/Kaique292/squadview/commit/080edd05a333ea57920cb3a402cf7dc49a18e507))
* **app:** mostra splash animada ao abrir o app ([eb937b5](https://github.com/Kaique292/squadview/commit/eb937b5900f2b6be16a813327d6d2e72d5bab719))
* **app:** transmite a tela e assiste os amigos dentro do app ([48c1b22](https://github.com/Kaique292/squadview/commit/48c1b226207677b74a1a6253179b10af6b8dee01))
* **app:** troca o ícone pelo mascote TV ([998605e](https://github.com/Kaique292/squadview/commit/998605ee40cfc6432489271ac6b42ddb041e36aa))
* **capture:** captura janela ou monitor com WGC e o áudio do jogo com WASAPI ([b77a713](https://github.com/Kaique292/squadview/commit/b77a71344cf849ba4193800f235d1a3937d2e3ce))
* **encode:** codifica H.264 por hardware via Media Foundation ([084c6bc](https://github.com/Kaique292/squadview/commit/084c6bc5f534bf53a7531d728c1ae06539aa7705))
* **media:** decodifica, desenha e toca o stream recebido ([7f3a4ee](https://github.com/Kaique292/squadview/commit/7f3a4ee0348672466da8f9f234a226d6e3eef6d6))
* **net:** conecta os membros direto por QUIC com hole punching ([6dd09b9](https://github.com/Kaique292/squadview/commit/6dd09b99f89f394e0b904f4a2345c733dd1fd647))
* **signal:** cria o esqueleto do worker de sinalização ([8fd0d1b](https://github.com/Kaique292/squadview/commit/8fd0d1bcaabd5c2a98746645450885bf27b1463a))
* **signal:** implementa a sala de sinalização no Durable Object ([a7c32fb](https://github.com/Kaique292/squadview/commit/a7c32fb6c17322e90ff5695620ab2336b52c4b71))
* **ui:** adiciona efeito de vidro com intensidade ajustável ([589c18a](https://github.com/Kaique292/squadview/commit/589c18a7ccc6456a7c8adeca8506f2ca8b71cdc0))
* **ui:** monta a interface da sala no estilo do Discord ([25d9882](https://github.com/Kaique292/squadview/commit/25d9882b3ffc691e381e49bf839c0058da0db211))


### 🐛 Correções

* **ui:** deixa a janela escura se a interface não carregar ([9b02fe7](https://github.com/Kaique292/squadview/commit/9b02fe702833a906517db36168eb1b9e1af5e662))


### 📝 Documentação

* adiciona documentação, ADRs, roadmap, design e imagens do projeto ([a5c088c](https://github.com/Kaique292/squadview/commit/a5c088c47b0b001e80edab9ec246dafa9c71c331))
* registra a atualização automática na ADR 0006 e nas docs ([59a5d8c](https://github.com/Kaique292/squadview/commit/59a5d8c2b6c4779d98becda066416e7e43d7a716))
* registra as ADRs de Media Foundation, mídia sobre QUIC e login com o Discord ([a7da0fc](https://github.com/Kaique292/squadview/commit/a7da0fc474094e42a545f37c6a9c718f9cefc1d1))


### 📦 Build

* configura workspace yarn, commitlint e hooks do git ([521a98f](https://github.com/Kaique292/squadview/commit/521a98f8207aee923540fdd7c450a34e8d6d24a8))
* **core:** cria workspace rust com os crates do núcleo ([e8df3c8](https://github.com/Kaique292/squadview/commit/e8df3c81a8b595adf24c815103dd9ae8237f2b57))
* **deps:** ajusta cargo-deny e manifesto do tauri após o primeiro build ([5d753b6](https://github.com/Kaique292/squadview/commit/5d753b61d008c1fba7bf7fbcb477bbf48b425d44))
* **deps:** libera a licença IJG só para o encoder das miniaturas ([7a7c1fe](https://github.com/Kaique292/squadview/commit/7a7c1fea30f768515c5e846032742f787ff21b4d))

## Changelog

Todas as mudanças relevantes do SquadView. Este arquivo é gerado pelo [release-please](https://github.com/googleapis/release-please) a partir dos [commits semânticos](docs/git-e-versionamento.md).
