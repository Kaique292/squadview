# 0001. Tauri + Rust no núcleo + React na interface

- **Status:** aceita
- **Data:** 2026-10-19

## Contexto

O app roda ao lado de jogos. O núcleo de mídia precisa de acesso direto a D3D11, NVENC e WASAPI, sem coletor de lixo e com custo previsível. A interface precisa ser bonita, rápida de evoluir e fácil para contribuidores.

## Decisão

Núcleo em **Rust** (workspace Cargo com crates `sv-*`), app em **Tauri v2** e interface em **React 19 + TypeScript + Vite**, com **Tailwind CSS v4**, **Radix UI**, **Zustand** e **lucide-react**.

## Consequências

- Instalador pequeno e pouca memória: Tauri usa o WebView2 do Windows em vez de embutir um navegador.
- O vídeo **não** passa pela WebView: é decodificado e desenhado pelo núcleo nativo, e a interface só controla.
- Contribuir na interface exige só TypeScript/React; no núcleo, Rust e conhecimento de Windows.

## Alternativas consideradas

- **Electron:** bem mais pesado em memória e disco, ruim para rodar ao lado de jogos.
- **C++ com Qt:** desempenho igual, mas segurança de memória pior e menos gente para contribuir.
- **Svelte/Solid:** mais leves, mas o ecossistema e o número de contribuidores em potencial são menores que o do React.
