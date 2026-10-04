##	Stops what a run started, by pid, and whatever those processes started in
##	turn. The boxes are shared, and a stop by name ended any SilkTerm on the
##	box along with whatever its panes were running.

##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

param([Parameter(Mandatory)] [string] $List)

if (-not (Test-Path $List)) { return }
$ours = foreach ($line in Get-Content $List) {
	$id, $ticks = $line -split ' '
	$p = Get-Process -Id $id -ErrorAction SilentlyContinue
	##	A pid is handed out again once its process ends, so the start time has to
	##	match too. Within a second, since off Windows it is worked out from the
	##	uptime and moves a little between two asks.
	if ($p -and [math]::Abs($p.StartTime.ToUniversalTime().Ticks - [long]$ticks) -lt [TimeSpan]::TicksPerSecond) { $p }
}
$all = Get-Process
$found = @{}
$todo = [System.Collections.Generic.Queue[object]]::new()
foreach ($p in $ours) { $todo.Enqueue($p) }
while ($todo.Count) {
	$p = $todo.Dequeue()
	if ($found.ContainsKey($p.Id)) { continue }
	$found[$p.Id] = $p
	foreach ($c in $all) {
		try {
			##	A child older than its parent names a pid that was reused.
			if ($c.Parent -and $c.Parent.Id -eq $p.Id -and $c.StartTime -ge $p.StartTime) { $todo.Enqueue($c) }
		} catch { }
	}
}
foreach ($p in $found.Values) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }

##	History:
##		- 20260918 JC: Created.
