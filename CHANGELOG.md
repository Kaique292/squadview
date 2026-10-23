# Changelog

## [0.11.1](https://github.com/Kaique292/squadview/compare/v0.11.0...v0.11.1) (2026-10-23)


### 🐛 Correções

* **app:** fliperama mais seguro e com menos atraso, e a live fecha com o programa ([#100](https://github.com/Kaique292/squadview/issues/100)) ([bd2d93a](https://github.com/Kaique292/squadview/commit/bd2d93a0502f7a5a26a26e674dc010758968853e))


### 📝 Documentação

* **docs:** atualiza o README e a captura do fliperama para a versão 0.11.1 ([#101](https://github.com/Kaique292/squadview/issues/101)) ([26276d0](https://github.com/Kaique292/squadview/commit/26276d0abbfda4fc7bc835d5912a6ccb970cc229))

## [0.11.0](https://github.com/Kaique292/squadview/compare/v0.10.0...v0.11.0) (2026-10-23)


### ✨ Novidades

* **app:** fliperama com controle, teclado e mouse dos jogadores ([#94](https://github.com/Kaique292/squadview/issues/94)) ([a686626](https://github.com/Kaique292/squadview/commit/a686626f96b15af7d0e909f59ec1b8c8c26a0031))
* **input:** cria o sv-input com controles virtuais e a prova do fliperama ([#90](https://github.com/Kaique292/squadview/issues/90)) ([191da6c](https://github.com/Kaique292/squadview/commit/191da6cc1e0cb160f3f68658e21c2ea53069d2a9))
* **net:** volta para a sala com o mesmo id depois de uma queda ([#86](https://github.com/Kaique292/squadview/issues/86)) ([6d4022c](https://github.com/Kaique292/squadview/commit/6d4022cc1f4457c32be2eb581d6eae1cbc3a6b26))


### 🐛 Correções

* **ci:** faz o -Coverage do clean-build apagar a cobertura inteira ([#88](https://github.com/Kaique292/squadview/issues/88)) ([58d7fd9](https://github.com/Kaique292/squadview/commit/58d7fd981e282d83df944f51de0201589ed1d4a8))
* **net:** conversa não para quando os dois amigos chamam ao mesmo tempo ([#92](https://github.com/Kaique292/squadview/issues/92)) ([bcffdb1](https://github.com/Kaique292/squadview/commit/bcffdb10ac4ba7288b86cb5d7fdabba691a851ca))
* **signal:** acaba com o clone na sala e espera o primeiro quadro para assistir ([#91](https://github.com/Kaique292/squadview/issues/91)) ([24f41b4](https://github.com/Kaique292/squadview/commit/24f41b40a8efcd031d13f1825e42e32d0971a647))
* **signal:** código de sala que não existe dá código inválido em vez de criar sala ([#93](https://github.com/Kaique292/squadview/issues/93)) ([3b2d186](https://github.com/Kaique292/squadview/commit/3b2d18682cf90f13241c5436d07c5b0bbf4e4102))


### 📝 Documentação

* **docs:** atualiza o README e as capturas para a versão 0.11 ([#97](https://github.com/Kaique292/squadview/issues/97)) ([cc62a44](https://github.com/Kaique292/squadview/commit/cc62a44f7c55efb8544677c009a1caf9e902efe7))
* **docs:** registra a ADR 0023 e o plano do modo Lan House Fliperama ([#89](https://github.com/Kaique292/squadview/issues/89)) ([57b11f7](https://github.com/Kaique292/squadview/commit/57b11f77d42ece1127c7f75d6c7ae9165ba74971))

## [0.10.0](https://github.com/Kaique292/squadview/compare/v0.9.0...v0.10.0) (2026-10-22)


### ✨ Novidades

* **app:** mostra a live por baixo da interface, com controles no picture in picture ([#81](https://github.com/Kaique292/squadview/issues/81)) ([06bcac8](https://github.com/Kaique292/squadview/commit/06bcac8be4e083ef053ec96b577dd34735d888d8))
* **app:** reações e respostas nas conversas, com a hora como no Discord ([#80](https://github.com/Kaique292/squadview/issues/80)) ([7b062a1](https://github.com/Kaique292/squadview/commit/7b062a1be6d17cfdb189e5b4cadb7a90d0db3f04))
* **ui:** sala como a do Discord, com espectadores no topo e a barra embaixo ([#79](https://github.com/Kaique292/squadview/issues/79)) ([1be8a58](https://github.com/Kaique292/squadview/commit/1be8a5879ad868146ed246fd77a41ecb4783a686))


### ♻️ Melhorias internas

* **media:** apaga o vídeo em janela por cima da interface ([#82](https://github.com/Kaique292/squadview/issues/82)) ([a9c01b9](https://github.com/Kaique292/squadview/commit/a9c01b997dca4b8495d3fee747f3d8378a6351ae))


### 📝 Documentação

* **docs:** atualiza o README e o roadmap para a versão 0.10 ([#83](https://github.com/Kaique292/squadview/issues/83)) ([e362200](https://github.com/Kaique292/squadview/commit/e362200edf6590f560f90fd3fde2660be5845c5e))

## [0.9.0](https://github.com/Kaique292/squadview/compare/v0.8.0...v0.9.0) (2026-10-21)


### ✨ Novidades

* **app:** assistir várias lives ao mesmo tempo, cada uma num quadro da sala ([#73](https://github.com/Kaique292/squadview/issues/73)) ([0dd3ef2](https://github.com/Kaique292/squadview/commit/0dd3ef2488f0ab691082a834ea20d13515bddc33))
* **app:** convite do servidor vira link clicável ([#70](https://github.com/Kaique292/squadview/issues/70)) ([dbe426a](https://github.com/Kaique292/squadview/commit/dbe426af208de21f485c1d0cf26464f1bd5e3cf3))
* **net:** voz repassada por um membro da sala quando dois não se alcançam ([#72](https://github.com/Kaique292/squadview/issues/72)) ([47ee234](https://github.com/Kaique292/squadview/commit/47ee234cf100dbad795d64d59dec293602569219))
* **signal:** sala sem limite de pessoas ([#74](https://github.com/Kaique292/squadview/issues/74)) ([dc51756](https://github.com/Kaique292/squadview/commit/dc517565f7dba4281ff21870e3eec12bfee4e80d))
* **ui:** conversas com cancelar download, aviso de arquivo apagado, som e contador ([#68](https://github.com/Kaique292/squadview/issues/68)) ([9670804](https://github.com/Kaique292/squadview/commit/9670804f6d305ecbc2dad05f1402e9a4ccde9edc))
* **ui:** mostra quem está em call no servidor ao entrar e no ícone lateral ([#69](https://github.com/Kaique292/squadview/issues/69)) ([5e20149](https://github.com/Kaique292/squadview/commit/5e201491455f31f79e2f6a8a647ec5269fa69dc7))


### 🐛 Correções

* **media:** silenciar e volume de alguém valem também depois que a voz dele reconecta ([#66](https://github.com/Kaique292/squadview/issues/66)) ([71d38f4](https://github.com/Kaique292/squadview/commit/71d38f40b75b0bd56dfd07b298bc5b422536505a))


### 📝 Documentação

* README, site e roadmap da versão 0.9 ([#75](https://github.com/Kaique292/squadview/issues/75)) ([178141c](https://github.com/Kaique292/squadview/commit/178141c7514998ef0203f06cf009c83d8ede26b7))


### 📦 Build

* limita o tamanho da target/ e documenta a limpeza ([#71](https://github.com/Kaique292/squadview/issues/71)) ([03fc6f7](https://github.com/Kaique292/squadview/commit/03fc6f7366dcd5fe1bbd424053cde81ff5d67a29))

## [0.8.0](https://github.com/Kaique292/squadview/compare/v0.7.0...v0.8.0) (2026-10-21)


### ✨ Novidades

* **signal:** presença nas salas do servidor e entrada direta para membros ([#61](https://github.com/Kaique292/squadview/issues/61)) ([be1ff9e](https://github.com/Kaique292/squadview/commit/be1ff9e50e89e2abed10bac780079bc5130edb49))
* **ui:** fundos em degradê, menu do servidor no botão direito e correções da live ([#60](https://github.com/Kaique292/squadview/issues/60)) ([2cb6f07](https://github.com/Kaique292/squadview/commit/2cb6f073081f0888ab216108a5f1b7c890555f2a))


### 🐛 Correções

* **net:** mensagem pendente tenta de novo quando o amigo fica online ([#62](https://github.com/Kaique292/squadview/issues/62)) ([a4ace95](https://github.com/Kaique292/squadview/commit/a4ace95574fe723d7525cd396ecf0c62b58afe08))


### 📝 Documentação

* README e site da versão 0.8 ([#63](https://github.com/Kaique292/squadview/issues/63)) ([14c3b20](https://github.com/Kaique292/squadview/commit/14c3b20843ac9f9de0753ee96ef91fbd6c4221ef))

## [0.7.0](https://github.com/Kaique292/squadview/compare/v0.6.0...v0.7.0) (2026-10-21)


### ✨ Novidades

* **ui:** abas com a barra deslizando e o conteúdo trocando com animação ([#52](https://github.com/Kaique292/squadview/issues/52)) ([7ef3f19](https://github.com/Kaique292/squadview/commit/7ef3f19d9a0e8c8068778e76ee77a6fd85f5dff3))
* **ui:** checkbox padrão do app, animado ao marcar e ao desmarcar ([#51](https://github.com/Kaique292/squadview/issues/51)) ([cde6f3b](https://github.com/Kaique292/squadview/commit/cde6f3b30e6dc81c7070629978953861a2099e92))
* **ui:** efeito de fala inteiro na lista e cores misturadas onde se tocam ([#53](https://github.com/Kaique292/squadview/issues/53)) ([d0331c7](https://github.com/Kaique292/squadview/commit/d0331c70f35c47b673ccb363c568213e4cda2537))
* **ui:** live como um quadro da sala, com menu e detalhes em gráficos ([#54](https://github.com/Kaique292/squadview/issues/54)) ([4cdfe88](https://github.com/Kaique292/squadview/commit/4cdfe882eb8ec0210042a3218625d3f6b0f46bf4))


### 🐛 Correções

* **app:** ícone do servidor volta a abrir a imagem escolhida ([#50](https://github.com/Kaique292/squadview/issues/50)) ([aebc53c](https://github.com/Kaique292/squadview/commit/aebc53c9def9cca0ce082d10290aa371a4a729ef))
* **ui:** rolagem das configurações com a barra do app, não a do Windows ([#48](https://github.com/Kaique292/squadview/issues/48)) ([01fd0a7](https://github.com/Kaique292/squadview/commit/01fd0a71d2b62306f03d9d595e7c228523966c1d))


### 📝 Documentação

* README e site da versão 0.7 ([#56](https://github.com/Kaique292/squadview/issues/56)) ([58db852](https://github.com/Kaique292/squadview/commit/58db8527b1069d88bf513689957fef54369666fc))

## [0.6.0](https://github.com/Kaique292/squadview/compare/v0.5.0...v0.6.0) (2026-10-21)


### ✨ Novidades

* **app:** limpa o cache do app inteiro ou de cada parte nas configurações ([#34](https://github.com/Kaique292/squadview/issues/34)) ([470f72a](https://github.com/Kaique292/squadview/commit/470f72aa2f300e67a29a7d989ace563396f636a1))
* **net:** conversa privada direto entre dois amigos, com arquivos ([#40](https://github.com/Kaique292/squadview/issues/40)) ([17b4407](https://github.com/Kaique292/squadview/commit/17b440798a8a89bc9e64a6a041a783d21fa71e7f))
* **signal:** repassa o sinal da conversa direta entre amigos ([#38](https://github.com/Kaique292/squadview/issues/38)) ([13c60c5](https://github.com/Kaique292/squadview/commit/13c60c58e812dadac6433ca8db6473c52265dcd6))
* **ui:** agrupa os botões da barra da chamada como no Discord ([#31](https://github.com/Kaique292/squadview/issues/31)) ([cdcbb0f](https://github.com/Kaique292/squadview/commit/cdcbb0fc31cd486973610c835e27834d816df018))
* **ui:** ajusta o ícone do servidor com recorte, posição e zoom ([#37](https://github.com/Kaique292/squadview/issues/37)) ([5ec83c3](https://github.com/Kaique292/squadview/commit/5ec83c33034a9e764431ff4a9e7db2cef47dc829))
* **ui:** assiste a live em picture in picture ([#35](https://github.com/Kaique292/squadview/issues/35)) ([fee614f](https://github.com/Kaique292/squadview/commit/fee614f564f1f3b3d7aa72c415a34c8ab0489ace))
* **ui:** conversas privadas na página de amigos, com anexos ([#41](https://github.com/Kaique292/squadview/issues/41)) ([4dc8419](https://github.com/Kaique292/squadview/commit/4dc8419cc94384adbe4ec8795653dc6761e23944))
* **ui:** efeitos de fala com cor e prévia, vistos por toda a sala ([#33](https://github.com/Kaique292/squadview/issues/33)) ([349c4b3](https://github.com/Kaique292/squadview/commit/349c4b3119d67ca4def48afb61b1b0684b57856d))
* **ui:** servidores com salas fixas, convites e papéis ([#36](https://github.com/Kaique292/squadview/issues/36)) ([002f0ae](https://github.com/Kaique292/squadview/commit/002f0ae8976e775cd7a175650218660cd108a8f0))


### 🐛 Correções

* **ui:** fechar a conversa não derruba mais a tela ([#43](https://github.com/Kaique292/squadview/issues/43)) ([f980bd6](https://github.com/Kaique292/squadview/commit/f980bd641a936364e1f4c07069050b4e961555a5))
* **ui:** recolhe os subitens de Voz e vídeo e desliza a barra do subitem ([#30](https://github.com/Kaique292/squadview/issues/30)) ([7e411d7](https://github.com/Kaique292/squadview/commit/7e411d7b01fffed240912b219dd83d750903c68c))
* **ui:** refaz a poeira ao parar de assistir no estilo do iOS ([#32](https://github.com/Kaique292/squadview/issues/32)) ([6e1149d](https://github.com/Kaique292/squadview/commit/6e1149d966ba2577299ce05b761a30abee1592b9))


### 📝 Documentação

* **docs:** atualiza o roadmap com o que já entrou até as conversas privadas ([#42](https://github.com/Kaique292/squadview/issues/42)) ([5b84332](https://github.com/Kaique292/squadview/commit/5b8433260cae8ecaab059c981b19c18be3ac2f60))
* README e site da versão 0.5 ([596f827](https://github.com/Kaique292/squadview/commit/596f8273659e79e748ae41be1390187acaa81510))
* README e site da versão 0.6 ([#44](https://github.com/Kaique292/squadview/issues/44)) ([ef80c70](https://github.com/Kaique292/squadview/commit/ef80c700b044035ca0c935ae8e7cfbf170433059))


### 📦 Build

* **ci:** release-please atualiza o Cargo.lock junto com os Cargo.toml ([70589b0](https://github.com/Kaique292/squadview/commit/70589b09f970cc5559011a5d891c18a4137e470b))

## [0.5.0](https://github.com/Kaique292/squadview/compare/v0.4.0...v0.5.0) (2026-10-21)


### ✨ Novidades

* **app:** fechar a janela manda o app para a bandeja ([2634bf8](https://github.com/Kaique292/squadview/commit/2634bf870a2f4e235e48756d0f8fd8cdd888742f))
* **app:** mostra quantos e quem estão assistindo cada live ([34b9c84](https://github.com/Kaique292/squadview/commit/34b9c847e6fde20c4843a57a1850829726638fd3))
* **net:** conexão em camadas para quem está atrás de CGNAT ([cf1fadd](https://github.com/Kaique292/squadview/commit/cf1fadd744367d8f92ab5d7e916c5a0684c6b7c1))
* **ui:** sons de aviso ao entrar, sair, mutar, ensurdecer e transmitir ([bf61141](https://github.com/Kaique292/squadview/commit/bf6114108bbf52baf5bb0365726b0c5ce5c43768))


### 🐛 Correções

* **app:** barra da abertura chega ao fim antes de abrir o app ([0d9a4de](https://github.com/Kaique292/squadview/commit/0d9a4ded3e4877e2b9cf76378d8c942ae1aa61f2))
* **app:** login com o Discord liga os amigos mesmo se a sessão demorar ([a8db0a3](https://github.com/Kaique292/squadview/commit/a8db0a332fb483c2249bbfbb8a2b302e244a13e7))
* **ui:** amigo que já está na sala não tem Entrar nem Chamar ([d02c057](https://github.com/Kaique292/squadview/commit/d02c0573fb58066be804ebad7fa6c31eb057ea9f))
* **ui:** mãozinha do mouse em tudo que é clicável ([f965328](https://github.com/Kaique292/squadview/commit/f965328e3420283c794ca349ebb477c0830c154b))
* **ui:** menus, dicas e avisos aparecem por cima da live sem escondê-la ([d18e684](https://github.com/Kaique292/squadview/commit/d18e6840c3713217e5088a2e35e25b0f4f2a59bb))
* **ui:** setinha dos participantes volta a funcionar e selo "Ao vivo" sem ícone ([327f2fc](https://github.com/Kaique292/squadview/commit/327f2fc68ca2b7ba24cb31696c981a4b2c4339ab))

## [0.4.0](https://github.com/Kaique292/squadview/compare/v0.3.1...v0.4.0) (2026-10-20)


### ✨ Novidades

* **mesh:** malha em enxame para salas sem limite, com simulador ([#16](https://github.com/Kaique292/squadview/issues/16)) ([b738435](https://github.com/Kaique292/squadview/commit/b738435d0f4501ad86f6a212ae086f6ada8074c5))


### 🐛 Correções

* **capture:** segue o jogo do cliente para a partida, em qualquer modo de tela ([#18](https://github.com/Kaique292/squadview/issues/18)) ([69ad2df](https://github.com/Kaique292/squadview/commit/69ad2df4740adb9fdef44e43d63b67b24a226126))
* certificados do rustls, amigos como início e atualizar na abertura ([#21](https://github.com/Kaique292/squadview/issues/21)) ([bbae696](https://github.com/Kaique292/squadview/commit/bbae69696d110ff58893a2c3f0a907d0f95d4eeb))
* voz no volume do Discord, grade em foco, atualizar como no Discord e amigos conferidos na sala ([#17](https://github.com/Kaique292/squadview/issues/17)) ([4047c0a](https://github.com/Kaique292/squadview/commit/4047c0ab25d441a2a86c680f348df28d638921a6))


### 📝 Documentação

* README e site com a voz em foco, o ganho automático e a captura que segue o jogo ([#19](https://github.com/Kaique292/squadview/issues/19)) ([d3dcf1c](https://github.com/Kaique292/squadview/commit/d3dcf1c48e1555e64ad3b82dfce98ab5605af3f6))
* README e site da versão 0.4 ([#22](https://github.com/Kaique292/squadview/issues/22)) ([0b01e37](https://github.com/Kaique292/squadview/commit/0b01e37916c702a3929edd8ccb996f04c85aef8d))

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
