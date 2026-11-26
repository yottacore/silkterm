##	PSScriptAnalyzer rules for the PowerShell lint, at warning level like the
##	shellcheck pass. Each rule left out here had only false hits in these scripts.
@{
	Severity     = @('Error', 'Warning')
	ExcludeRules = @(
		##	Console scripts, whose output is for the person watching.
		'PSAvoidUsingWriteHost'
		##	Functions here are fCamelCase, the house bash style, not Verb-Noun. An
		##	exception for this project only.
		'PSUseApprovedVerbs'
		##	A byte-order mark breaks `irm | iex` and a shebang line.
		'PSUseBOMForUnicodeEncodedFile'
		##	It follows no value from script scope into a function or a script
		##	block, so parameters and settings read there look unused.
		'PSReviewUnusedParameter'
		'PSUseDeclaredVarsMoreThanAssignments'
		##	The prompt hook in shell_integration.ps1 keeps its state between prompts.
		'PSAvoidGlobalVars'
		##	Best-effort cleanup and probes, where a failure has nothing to report.
		'PSAvoidUsingEmptyCatchBlock'
	)
	Rules        = @{
		##	Set for tabs but off. It reads spaces that line up a continuation
		##	after the tabs as bad indentation, and wants one level per opener,
		##	so `{ @(` on one line counts twice. About 70 lines in 8 scripts
		##	would change. ps-lint.ps1 checks for tabs instead.
		PSUseConsistentIndentation = @{
			Enable              = $false
			Kind                = 'tab'
			IndentationSize     = 4
			PipelineIndentation = 'IncreaseIndentationForFirstPipeline'
		}
	}
}
