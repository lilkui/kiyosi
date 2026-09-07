[CmdletBinding()]
param(
    [string]$DerivaSharpRoot = (Join-Path $PSScriptRoot '..\..\DerivaSharp'),
    [string]$OutputPath = (Join-Path $PSScriptRoot '..\tests\fixtures\cpu_parity.tsv')
)

$ErrorActionPreference = 'Stop'
$pinnedRevision = '08efb5a0f0f308c0ab7c1a82f1ece1bf63b09fd2'
$actualRevision = (git -C $DerivaSharpRoot rev-parse HEAD).Trim()
if ($actualRevision -ne $pinnedRevision) {
    throw "DerivaSharp checkout must be at $pinnedRevision (found $actualRevision)"
}

$sourceFiles = @{
    'Vanilla.EuropeanOptionTestData.ValueData' = 'tests/DerivaSharp.Tests/Vanilla/EuropeanOptionTestData.cs'
    'Vanilla.EuropeanOptionTestData.GreekData' = 'tests/DerivaSharp.Tests/Vanilla/EuropeanOptionTestData.cs'
    'Vanilla.AmericanOptionTestData.ValueData' = 'tests/DerivaSharp.Tests/Vanilla/AmericanOptionTestData.cs'
    'Digital.DigitalOptionTestData.ValueData' = 'tests/DerivaSharp.Tests/Digital/DigitalOptionTestData.cs'
    'Barrier.BarrierOptionTestData.ValueData' = 'tests/DerivaSharp.Tests/Barrier/BarrierOptionTestData.cs'
    'Barrier.BarrierOptionTestData.FdParameters' = 'tests/DerivaSharp.Tests/Barrier/BarrierOptionTestData.cs'
    'Digital.BinaryBarrierOptionTestData.ValueData' = 'tests/DerivaSharp.Tests/Digital/BinaryBarrierOptionTestData.cs'
    'Asian.AsianOptionTestData.ArithmeticValueData' = 'tests/DerivaSharp.Tests/Asian/AsianOptionTestData.cs'
    'Asian.GeometricAverageAsianEngineTest.Value_IsAccurate' = 'tests/DerivaSharp.Tests/Asian/GeometricAverageAsianEngineTest.cs'
    'Autocallable.FdPhoenixEngineTest.StandardPhoenixValue_IsAccurate' = 'tests/DerivaSharp.Tests/Autocallable/FdPhoenixEngineTest.cs'
    'Autocallable.McPhoenixEngineTest.StandardPhoenixValue_IsAccurate' = 'tests/DerivaSharp.MonteCarlo.Tests/Autocallable/McPhoenixEngineTest.cs'
    'Autocallable.McSnowballEngineTest.StandardSnowballValue_IsAccurate' = 'tests/DerivaSharp.MonteCarlo.Tests/Autocallable/McSnowballEngineTest.cs'
    'Autocallable.McBinarySnowballEngineTest.BinarySnowballValue_KnockOutOnObservationDate_IsAccurate' = 'tests/DerivaSharp.MonteCarlo.Tests/Autocallable/McBinarySnowballEngineTest.cs'
    'Autocallable.McTernarySnowballEngineTest.TernarySnowballValue_KnockOutOnObservationDate_IsAccurate' = 'tests/DerivaSharp.MonteCarlo.Tests/Autocallable/McTernarySnowballEngineTest.cs'
    'Accumulator.FdAccumulatorEngineTest' = 'tests/DerivaSharp.MonteCarlo.Tests/Accumulator/FdAccumulatorEngineTest.cs'
}

$text = Get-Content -Raw -LiteralPath $OutputPath
$rows = $text -split "`r?`n"
for ($index = 0; $index -lt $rows.Count; $index++) {
    if (!$rows[$index] -or $rows[$index].StartsWith('#') -or $rows[$index].StartsWith('case_id')) { continue }
    $fields = $rows[$index] -split "`t", 10
    if ($fields.Count -ne 10) { throw "fixture row $($index + 1) must have 10 columns" }
    $inputs = [ordered]@{}
    foreach ($entry in ($fields[4] -split ';')) {
        if ($entry -eq '-') { continue }
        $pair = $entry -split '=', 2
        if ($pair.Count -eq 2) { $inputs[$pair[0]] = $pair[1] }
    }
    if (!$inputs.Contains('source_revision')) { $inputs['source_revision'] = $pinnedRevision }
    $symbol = switch -Regex ($fields[0]) {
            '^european-analytic$' { 'Vanilla.EuropeanOptionTestData.ValueData'; break }
            '^american-' { 'Vanilla.AmericanOptionTestData.ValueData'; break }
            '^cash-digital|^asset-digital|^digital-' { 'Digital.DigitalOptionTestData.ValueData'; break }
            '^barrier-' { if ($fields[2] -like '*FiniteDifference*') { 'Barrier.BarrierOptionTestData.FdParameters' } else { 'Barrier.BarrierOptionTestData.ValueData' }; break }
            '^binary-barrier' { 'Digital.BinaryBarrierOptionTestData.ValueData'; break }
            '^asian-arithmetic$' { 'Asian.AsianOptionTestData.ArithmeticValueData'; break }
            '^asian-geometric' { 'Asian.GeometricAverageAsianEngineTest.Value_IsAccurate'; break }
            '^accumulator-' { 'Accumulator.FdAccumulatorEngineTest'; break }
            '^phoenix-' { 'Autocallable.McPhoenixEngineTest.StandardPhoenixValue_IsAccurate'; break }
            '^snowball-' { 'Autocallable.McSnowballEngineTest.StandardSnowballValue_IsAccurate'; break }
            '^binary-snowball-' { 'Autocallable.McBinarySnowballEngineTest.BinarySnowballValue_KnockOutOnObservationDate_IsAccurate'; break }
            '^ternary-snowball-' { 'Autocallable.McTernarySnowballEngineTest.TernarySnowballValue_KnockOutOnObservationDate_IsAccurate'; break }
            '^structured-fd$' { 'Autocallable.FdPhoenixEngineTest.StandardPhoenixValue_IsAccurate'; break }
            '^european-' { 'Vanilla.EuropeanOptionTestData.ValueData'; break }
            default { 'Vanilla.EuropeanOptionTestData.ValueData' }
        }
    $inputs['source_symbol'] = $symbol
    $inputs['convention'] = 'Actual/365 Fixed, continuously compounded BSM'
    if (!$inputs.Contains('reference_kind')) {
        $inputs['reference_kind'] = if ($fields[2] -like '*MonteCarlo*') { 'statistical' } elseif ($fields[2] -like '*FiniteDifference*' -or $fields[8] -ne '-') { 'discretized' } else { 'analytic' }
    }
    $fields[4] = (($inputs.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ';')
    foreach ($column in 5, 6) {
        if ($fields[$column] -eq '-') { continue }
        $fields[$column] = (($fields[$column] -split ';' | ForEach-Object {
            $pair = $_ -split '=', 2
            if ($pair.Count -eq 2) {
                $number = 0.0
                if ([double]::TryParse($pair[1], [Globalization.NumberStyles]::Float,
                                       [Globalization.CultureInfo]::InvariantCulture, [ref]$number)) {
                    "$($pair[0])=$($number.ToString('0.000000', [Globalization.CultureInfo]::InvariantCulture))"
                } else { $_ }
            } else { $_ }
        }) -join ';')
    }
    if ($fields[6] -ne '-') {
        $firstTolerance = ($fields[6] -split ';' | Select-Object -First 1) -split '=', 2
        $inputs['tolerance'] = $firstTolerance[1]
    } else {
        $inputs['tolerance'] = '0.000000'
    }
    $fields[4] = (($inputs.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ';')
    $rows[$index] = $fields -join "`t"
}

foreach ($symbol in $sourceFiles.Keys) {
    $path = Join-Path $DerivaSharpRoot $sourceFiles[$symbol]
    if (!(Test-Path -LiteralPath $path)) { throw "missing pinned source file for $symbol`: $path" }
    $source = Get-Content -Raw -LiteralPath $path
    $member = $symbol.Split('.')[-1]
    if ($source -notmatch [regex]::Escape($member)) { throw "pinned source symbol $symbol was not found in $path" }
}

[IO.File]::WriteAllText((Resolve-Path $OutputPath), (($rows -join "`r`n").TrimEnd() + "`r`n"))
Write-Output "Updated $OutputPath from DerivaSharp $pinnedRevision"
