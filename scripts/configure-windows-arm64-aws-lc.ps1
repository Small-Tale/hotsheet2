# HS2-XW91TJ, HS2-HK3RMC: aws-lc-sys and rav1e need clang-cl for Windows ARM
# assembly. rav1e uses cc-rs and inherits CC; MSVC cl silently ignores its .S files.
$ErrorActionPreference = 'Stop'

$candidates = @()
if ($env:VSINSTALLDIR) {
  $candidates += Join-Path $env:VSINSTALLDIR 'VC/Tools/Llvm/ARM64/bin/clang-cl.exe'
  $candidates += Join-Path $env:VSINSTALLDIR 'VC/Tools/Llvm/x64/bin/clang-cl.exe'
}
$candidates += @(Get-Command clang-cl.exe -All -CommandType Application -ErrorAction SilentlyContinue |
  Select-Object -ExpandProperty Source)

$probe = Join-Path $env:RUNNER_TEMP 'hs2-aws-lc-arm64-probe.S'
$object = Join-Path $env:RUNNER_TEMP 'hs2-aws-lc-arm64-probe.obj'
@('.text', '.globl hs2_aws_lc_arm64_probe', 'hs2_aws_lc_arm64_probe:', 'ret') |
  Set-Content -LiteralPath $probe -Encoding ascii

$compiler = $null
foreach ($candidate in ($candidates | Select-Object -Unique)) {
  if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) { continue }
  Remove-Item -LiteralPath $object -ErrorAction SilentlyContinue
  & $candidate --target=aarch64-pc-windows-msvc /c $probe "/Fo$object"
  if ($LASTEXITCODE -eq 0 -and (Test-Path -LiteralPath $object -PathType Leaf)) {
    $compiler = $candidate
    break
  }
}

if (-not $compiler) {
  throw 'No clang-cl.exe could assemble a Windows ARM64 object; install the Visual Studio C++ Clang Compiler for Windows component'
}

Write-Output "Using clang-cl for Windows ARM64 assembly: $compiler"
"AWS_LC_SYS_CC=$compiler" | Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8
"CC=$compiler" | Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8
