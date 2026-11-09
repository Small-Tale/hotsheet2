# HS2-BCCF8J: dav1d's MSVC ARM assembly build requires this Perl helper on PATH.
$ErrorActionPreference = 'Stop'
$commit = 'd09971fad329d32df19f5bbafe88cf2f0ed04ed7'
$expectedSha256 = '78ad06351ac8be513fb6e6291d9bb8f667899625c2649093e3b08f59fae950a2'
$directory = Join-Path $env:RUNNER_TEMP 'gas-preprocessor'
$script = Join-Path $directory 'gas-preprocessor.pl'

New-Item -ItemType Directory -Path $directory -Force | Out-Null
Invoke-WebRequest -Uri "https://raw.githubusercontent.com/libav/gas-preprocessor/$commit/gas-preprocessor.pl" -OutFile $script
$digest = (Get-FileHash -Algorithm SHA256 -Path $script).Hash.ToLowerInvariant()
if ($digest -ne $expectedSha256) {
  throw 'Downloaded gas-preprocessor does not match the pinned upstream SHA-256'
}
# HS2-MX0N0G: upstream converts tbz numeric targets but leaves tbnz targets in
# GNU form, which armasm64 rejects in dav1d's loopfilter assembly.
$oldBranchPattern = 'cbn?z|adr|tbz'
$newBranchPattern = 'cbn?z|adr|tbn?z'
$patchedSha256 = '1b4a7519ff4746ca26a6129bb799356d2d1394e68daff2fc693d8df37030fc99'
$source = [System.IO.File]::ReadAllText($script)
if ($source.IndexOf($oldBranchPattern) -lt 0 -or $source.IndexOf($oldBranchPattern) -ne $source.LastIndexOf($oldBranchPattern)) {
  throw 'Pinned gas-preprocessor branch pattern is missing or ambiguous'
}
$source = $source.Replace($oldBranchPattern, $newBranchPattern)
[System.IO.File]::WriteAllText($script, $source, [System.Text.UTF8Encoding]::new($false))
if ((Get-FileHash -Algorithm SHA256 -Path $script).Hash.ToLowerInvariant() -ne $patchedSha256) {
  throw 'Patched gas-preprocessor does not match the expected SHA-256'
}
perl -c $script
if ($LASTEXITCODE -ne 0) { throw 'Pinned gas-preprocessor failed Perl syntax validation' }
$directory | Out-File -FilePath $env:GITHUB_PATH -Append -Encoding utf8
