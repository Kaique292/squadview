<#
.SYNOPSIS
  Teste do clean-build.ps1: roda o script numa cópia do projeto em uma pasta temporária, com uma
  target/ de mentira, e confere o que cada parâmetro apaga e o que fica.
#>
$ErrorActionPreference = "Stop"

$root = Join-Path ([System.IO.Path]::GetTempPath()) ("clean-build-" + [guid]::NewGuid())
$falhas = 0

function Arquivo([string]$path) {
  New-Item -ItemType Directory -Force (Split-Path -Parent $path) | Out-Null
  Set-Content -Path $path -Value "x"
}

function Confere([string]$descricao, [bool]$ok) {
  if ($ok) { "ok    $descricao" } else { "FALHA $descricao"; $script:falhas++ }
}

try {
  New-Item -ItemType Directory -Force (Join-Path $root "scripts") | Out-Null
  Copy-Item (Join-Path $PSScriptRoot "clean-build.ps1") (Join-Path $root "scripts")
  $script = Join-Path $root "scripts/clean-build.ps1"
  $debug = Join-Path $root "target/debug/deps/libalgo.rlib"
  # O llvm-cov guarda as dependências instrumentadas, não só os crates do projeto.
  $cobertura = Join-Path $root "target/llvm-cov-target/debug/deps/libdependencia.rlib"
  $perfil = Join-Path $root "target/llvm-cov-target/squadview-123.profraw"
  $codigo = Join-Path $root "src/main.rs"
  Arquivo $debug
  Arquivo $cobertura
  Arquivo $perfil
  Arquivo $codigo

  # Sem parâmetros: só mostra.
  & $script | Out-Null
  Confere "sem parâmetros não apaga nada" ((Test-Path $cobertura) -and (Test-Path $debug))

  # -Coverage: a cobertura inteira some, inclusive as dependências; o resto fica.
  & $script -Coverage | Out-Null
  Confere "-Coverage apaga a target/llvm-cov-target inteira" (-not (Test-Path (Join-Path $root "target/llvm-cov-target")))
  Confere "-Coverage deixa a target/debug" (Test-Path $debug)
  Confere "-Coverage não mexe fora da target/" (Test-Path $codigo)

  # De novo, já sem a pasta: não dá erro.
  & $script -Coverage | Out-Null
  Confere "-Coverage sem a pasta não quebra" $true
} finally {
  Remove-Item $root -Recurse -Force -ErrorAction SilentlyContinue
}

if ($falhas -gt 0) { throw "$falhas verificação(ões) do clean-build.ps1 falharam" }
