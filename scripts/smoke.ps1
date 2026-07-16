param(
    [string]$TargetDir = "C:\tmp\bbarit-agent-target",
    [string]$SmokeRoot = "C:\tmp\bbarit-smoke"
)

$ErrorActionPreference = "Stop"

function Invoke-NativeChecked([string]$Command, [string[]]$Arguments = @()) {
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Command failed with exit code $LASTEXITCODE"
    }
}
function Get-PlainTerminalText([string]$Path) {
    $raw = Get-Content -Path $Path -Raw
    $esc = [regex]::Escape(([char]27).ToString())
    return [regex]::Replace($raw, "$esc\[[0-9;?]*[ -/]*[@-~]", "")
}

function Assert-TerminalText([string]$Path, [string]$Pattern, [string]$Message) {
    $plain = Get-PlainTerminalText $Path
    if ($plain -notmatch $Pattern) {
        throw $Message
    }
}

$repo = Split-Path -Parent $PSScriptRoot
Push-Location $repo
try {
    $env:CARGO_TARGET_DIR = $TargetDir
    Remove-Item -LiteralPath $SmokeRoot -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force -Path $SmokeRoot | Out-Null
    $env:PI_CODING_AGENT_DIR = Join-Path $SmokeRoot "agent"

    Write-Host "== cargo check =="
    Invoke-NativeChecked "cargo" @("check")

    Write-Host "== cargo test =="
    Invoke-NativeChecked "cargo" @("test")

    Write-Host "== cargo build =="
    Invoke-NativeChecked "cargo" @("build")

    $exe = Join-Path $TargetDir "debug\bbarit.exe"

    Write-Host "== bbarit --version =="
    & $exe --version

    Write-Host "== slash smoke =="
    $slashSession = & $exe --no-session --print /session
    $slashSession
    $slashSessionText = $slashSession -join "`n"
    if ($slashSessionText -notmatch "File:\s+in-memory" -or $slashSessionText -notmatch "Estimated tokens:" -or $slashSessionText -notmatch "Actual tokens:" -or $slashSessionText -notmatch "Last model usage:" -or $slashSessionText -notmatch "Cost:\s+0\.0000") {
        throw "/session did not show file/tokens/cost stats"
    }
    $ephemeralId = "ephemeral-fixed-session"
    $ephemeralSessionDir = Join-Path $SmokeRoot "ephemeral-session-dir"
    $ephemeralSession = & $exe --no-session --session-id $ephemeralId --session-dir $ephemeralSessionDir --print /session
    if (($ephemeralSession -join "`n") -notmatch "Session $ephemeralId") {
        throw "--no-session --session-id did not use deterministic session id"
    }
    if (Test-Path (Join-Path $ephemeralSessionDir "$ephemeralId.jsonl")) {
        throw "--no-session --session-id wrote a session file"
    }
    & $exe --no-session --print /providers | Select-Object -First 5
    $ollamaModels = & $exe --no-session --print "/models ollama"
    if (($ollamaModels -join "`n") -notmatch "ollama") {
        throw "ollama provider models did not load"
    }
    $ollamaShortcut = & $exe --no-session --print "/ollama"
    if (($ollamaShortcut -join "`n") -notmatch "Ollama models:" -or ($ollamaShortcut -join "`n") -notmatch "CLI: run /model ollama/") {
        throw "/ollama shortcut did not list Ollama models"
    }
    & $exe --no-session --print /hotkeys | Select-Object -First 5
    $changelogOutput = & $exe --no-session --print /changelog
    if (($changelogOutput -join "`n") -notmatch "What's New" -or ($changelogOutput -join "`n") -notmatch "0\.80\.") {
        throw "/changelog did not show bundled changelog entries"
    }
    $quitOutput = & $exe --no-session --print /quit
    if (($quitOutput -join "`n") -notmatch "Quit") {
        throw "/quit did not run through command mode"
    }
    $reloadOutput = & $exe --no-session --print /reload
    $reloadText = $reloadOutput -join "`n"
    if ($reloadText -notmatch "Reloaded dynamic resources" -or $reloadText -notmatch "prompts\s+\d+" -or $reloadText -notmatch "themes\s+\d+" -or $reloadText -notmatch "extensions\s+\d+") {
        throw "/reload did not report resource counts"
    }
    & $exe --no-session --print /settings | Select-Object -First 8

    Write-Host "== json mode smoke =="
    $jsonOutput = & $exe --mode json --no-session /settings
    $jsonEvents = $jsonOutput | ForEach-Object { $_ | ConvertFrom-Json }
    $jsonTypes = @($jsonEvents | ForEach-Object { $_.type })
    foreach ($requiredType in @("session", "agent_start", "turn_start", "message_end", "turn_end", "agent_end")) {
        if ($jsonTypes -notcontains $requiredType) {
            throw "json mode did not emit $requiredType"
        }
    }
    if ($jsonEvents[0].version -ne 3) {
        throw "json mode session header did not report version 3"
    }

    Write-Host "== system prompt file smoke =="
    $systemPromptFile = Join-Path $SmokeRoot "system-prompt-file.txt"
    $appendSystemPromptFile = Join-Path $SmokeRoot "append-system-prompt-file.txt"
    ("system prompt file content " * 6) | Set-Content -Path $systemPromptFile -Encoding UTF8
    ("append system prompt file content " * 6) | Set-Content -Path $appendSystemPromptFile -Encoding UTF8
    $promptSettings = & $exe --no-session --system-prompt $systemPromptFile --append-system-prompt $appendSystemPromptFile --print /settings
    $promptSettingsText = $promptSettings -join "`n"
    if ($promptSettingsText -notmatch "system_prompt_bytes\s+1[0-9][0-9]") {
        throw "--system-prompt file path was not resolved to file contents"
    }
    if ($promptSettingsText -notmatch "append_system_prompt_count\s+1") {
        throw "--append-system-prompt did not register one append"
    }
    if ($promptSettingsText -notmatch "append_system_prompt_bytes\s+[1-9][0-9][0-9]") {
        throw "--append-system-prompt file path was not resolved to file contents"
    }
    $systemPromptProject = Join-Path $SmokeRoot "system-prompt-project"
    New-Item -ItemType Directory -Force -Path (Join-Path $systemPromptProject ".pi") | Out-Null
    ("project SYSTEM.md content " * 6) | Set-Content -Path (Join-Path $systemPromptProject ".pi\SYSTEM.md") -Encoding UTF8
    ("project APPEND_SYSTEM.md content " * 6) | Set-Content -Path (Join-Path $systemPromptProject ".pi\APPEND_SYSTEM.md") -Encoding UTF8
    Push-Location $systemPromptProject
    try {
        $untrustedPromptSettings = & $exe --no-approve --no-session --print /settings
        if (($untrustedPromptSettings -join "`n") -notmatch "system_prompt_bytes\s+0") {
            throw "untrusted .pi/SYSTEM.md was loaded"
        }
        $projectPromptSettings = & $exe --approve --no-session --print /settings
        $projectPromptSettingsText = $projectPromptSettings -join "`n"
        if ($projectPromptSettingsText -notmatch "system_prompt_bytes\s+[1-9][0-9][0-9]") {
            throw ".pi/SYSTEM.md was not discovered"
        }
        if ($projectPromptSettingsText -notmatch "append_system_prompt_count\s+1") {
            throw ".pi/APPEND_SYSTEM.md was not discovered"
        }
        if ($projectPromptSettingsText -notmatch "append_system_prompt_bytes\s+[1-9][0-9][0-9]") {
            throw ".pi/APPEND_SYSTEM.md contents were not loaded"
        }
    } finally {
        Pop-Location
    }

    Write-Host "== rpc mode smoke =="
    $rpcThemeDir = Join-Path $env:PI_CODING_AGENT_DIR "themes"
    $rpcThemePath = Join-Path $rpcThemeDir "rpc-theme.json"
    New-Item -ItemType Directory -Force -Path $rpcThemeDir | Out-Null
    @{
        name = "rpc-theme"
        colors = @{
            accent = "#123456"
            border = "#654321"
            muted = "#777777"
            text = "#ffffff"
        }
    } | ConvertTo-Json -Depth 5 | Set-Content -Path $rpcThemePath -Encoding UTF8
    $rpcInput = @(
        '{"id":"state","type":"get_state"}'
        '{"id":"models","type":"get_available_models"}'
        '{"id":"themes","type":"get_themes"}'
        '{"id":"name","type":"set_session_name","name":"RPC Smoke"}'
        '{"id":"bash","type":"bash","command":"Write-Output rpc-bash"}'
        '{"id":"prompt","type":"prompt","message":"/settings"}'
        '{"id":"commands","type":"get_commands"}'
    )
    $rpcOutput = $rpcInput | & $exe --mode rpc --no-session --approve
    if ($LASTEXITCODE -ne 0) {
        throw "rpc mode exited with $LASTEXITCODE"
    }
    $rpcEvents = $rpcOutput | ForEach-Object { $_ | ConvertFrom-Json }
    $rpcTypes = @($rpcEvents | ForEach-Object { $_.type })
    foreach ($requiredType in @("session", "response", "agent_start", "turn_start", "message_end", "turn_end", "agent_end")) {
        if ($rpcTypes -notcontains $requiredType) {
            throw "rpc mode did not emit $requiredType"
        }
    }
    $rpcPromptResponses = @($rpcEvents | Where-Object { $_.type -eq "response" -and $_.command -eq "prompt" })
    if ($rpcPromptResponses.Count -ne 1 -or -not $rpcPromptResponses[0].success) {
        throw "rpc prompt did not emit one successful acceptance response"
    }
    $rpcState = $rpcEvents | Where-Object { $_.type -eq "response" -and $_.command -eq "get_state" } | Select-Object -First 1
    if (-not $rpcState.success -or -not $rpcState.data.sessionId) {
        throw "rpc get_state did not return session state"
    }
    $rpcModels = $rpcEvents | Where-Object { $_.type -eq "response" -and $_.command -eq "get_available_models" } | Select-Object -First 1
    if (-not $rpcModels.success -or $rpcModels.data.models.Count -lt 100) {
        throw "rpc get_available_models returned too few models"
    }
    $rpcThemes = $rpcEvents | Where-Object { $_.type -eq "response" -and $_.command -eq "get_themes" } | Select-Object -First 1
    $rpcThemeMatches = @($rpcThemes.data.themes | Where-Object { $_.name -eq "rpc-theme" })
    if (-not $rpcThemes.success -or $rpcThemeMatches.Count -ne 1) {
        throw "rpc get_themes did not return user theme"
    }
    $rpcBash = $rpcEvents | Where-Object { $_.type -eq "response" -and $_.command -eq "bash" } | Select-Object -First 1
    if (-not $rpcBash.success -or $rpcBash.data.output -notmatch "rpc-bash") {
        throw "rpc bash did not execute"
    }
    $rpcCommands = $rpcEvents | Where-Object { $_.type -eq "response" -and $_.command -eq "get_commands" } | Select-Object -First 1
    if (-not $rpcCommands.success) {
        throw "rpc get_commands did not return successfully"
    }

    Write-Host "== tool flags smoke =="
    $noToolsSettings = & $exe --no-tools --no-session --print /settings
    if (($noToolsSettings -join "`n") -notmatch "no_tools\s+true") {
        throw "--no-tools did not set no_tools"
    }
    $shortNoToolsSettings = & $exe -nt --no-session --print /settings
    if (($shortNoToolsSettings -join "`n") -notmatch "no_tools\s+true") {
        throw "-nt did not set no_tools"
    }
    $noBuiltinToolsSettings = & $exe --no-builtin-tools --no-session --print /settings
    if (($noBuiltinToolsSettings -join "`n") -notmatch "no_tools\s+false" -or ($noBuiltinToolsSettings -join "`n") -notmatch "no_builtin_tools\s+true") {
        throw "--no-builtin-tools did not preserve extension/custom tools"
    }
    $toolFilterSettings = & $exe --tools read,grep -xt bash --no-session --print /settings
    if (($toolFilterSettings -join "`n") -notmatch "tools\s+read, grep") {
        throw "--tools did not set tool allowlist"
    }
    if (($toolFilterSettings -join "`n") -notmatch "exclude_tools\s+bash") {
        throw "-xt did not set tool exclude list"
    }

    Write-Host "== resource cli flags smoke =="
    $resourceFlagProject = Join-Path $SmokeRoot "resource-cli-flags-project"
    $settingsExtension = Join-Path $resourceFlagProject "settings-extension"
    $explicitExtension = Join-Path $resourceFlagProject "explicit-extension"
    $settingsPrompts = Join-Path $resourceFlagProject "settings-prompts"
    $explicitPrompts = Join-Path $resourceFlagProject "explicit-prompts"
    $settingsSkill = Join-Path $resourceFlagProject "settings-skills\settings-skill"
    $explicitSkill = Join-Path $resourceFlagProject "explicit-skills\explicit-skill"
    $explicitTheme = Join-Path $resourceFlagProject "explicit-theme.json"
    New-Item -ItemType Directory -Force -Path (Join-Path $resourceFlagProject ".pi") | Out-Null
    New-Item -ItemType Directory -Force -Path $settingsExtension,$explicitExtension,$settingsPrompts,$explicitPrompts,$settingsSkill,$explicitSkill | Out-Null
    "project context" | Set-Content -Path (Join-Path $resourceFlagProject "AGENTS.md") -Encoding UTF8
    @{
        extensions = @("./settings-extension")
        prompts = @("./settings-prompts")
        skills = @("./settings-skills")
        themes = @("./settings-theme.json")
    } | ConvertTo-Json | Set-Content -Path (Join-Path $resourceFlagProject ".pi\settings.json") -Encoding UTF8
    @{ id = "settings-ext"; name = "Settings Extension"; commands = @{} } | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $settingsExtension "extension.json") -Encoding UTF8
    @{ id = "explicit-ext"; name = "Explicit Extension"; commands = @{} } | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $explicitExtension "extension.json") -Encoding UTF8
    "---`ndescription: Settings prompt`n---`nsettings" | Set-Content -Path (Join-Path $settingsPrompts "settings-prompt.md") -Encoding UTF8
    "---`ndescription: Explicit prompt`n---`nexplicit" | Set-Content -Path (Join-Path $explicitPrompts "explicit-prompt.md") -Encoding UTF8
    "---`nname: settings-skill`ndescription: Settings skill`n---`nsettings" | Set-Content -Path (Join-Path $settingsSkill "SKILL.md") -Encoding UTF8
    "---`nname: explicit-skill`ndescription: Explicit skill`n---`nexplicit" | Set-Content -Path (Join-Path $explicitSkill "SKILL.md") -Encoding UTF8
    "{}" | Set-Content -Path $explicitTheme -Encoding UTF8
    Push-Location $resourceFlagProject
    try {
        $onlyExplicitExtension = & $exe --approve -ne -e $explicitExtension --no-session --print /extensions
        if (($onlyExplicitExtension -join "`n") -notmatch "explicit-ext") {
            throw "-ne -e did not load explicit extension"
        }
        if (($onlyExplicitExtension -join "`n") -match "settings-ext") {
            throw "-ne did not suppress settings extension"
        }
        $onlyExplicitPrompts = & $exe --approve -np --prompt-template $explicitPrompts --no-session --print /prompts
        if (($onlyExplicitPrompts -join "`n") -notmatch "explicit-prompt") {
            throw "-np --prompt-template did not load explicit prompt"
        }
        if (($onlyExplicitPrompts -join "`n") -match "settings-prompt") {
            throw "-np did not suppress settings prompt"
        }
        $onlyExplicitSkills = & $exe --approve -ns --skill $explicitSkill --no-session --print /skills
        if (($onlyExplicitSkills -join "`n") -notmatch "explicit-skill") {
            throw "-ns --skill did not load explicit skill"
        }
        if (($onlyExplicitSkills -join "`n") -match "settings-skill") {
            throw "-ns did not suppress settings skill"
        }
        $resourceSettings = & $exe --approve -nc --no-themes --theme $explicitTheme --no-session --print /settings
        if (($resourceSettings -join "`n") -notmatch "no_context_files\s+true") {
            throw "-nc did not set no_context_files"
        }
        if (($resourceSettings -join "`n") -notmatch "no_themes\s+true") {
            throw "--no-themes did not set no_themes"
        }
        if (($resourceSettings -join "`n") -notmatch [regex]::Escape($explicitTheme)) {
            throw "--theme did not preserve explicit theme path"
        }
    } finally {
        Pop-Location
    }

    Write-Host "== package cli smoke =="
    $packageCliProject = Join-Path $SmokeRoot "package-cli-project"
    $packageCliSource = Join-Path $SmokeRoot "package-cli-source"
    New-Item -ItemType Directory -Force -Path $packageCliProject | Out-Null
    New-Item -ItemType Directory -Force -Path $packageCliSource | Out-Null
    Push-Location $packageCliProject
    try {
        $installUser = & $exe install $packageCliSource
        if (($installUser -join "`n") -notmatch "Added package to user settings") {
            throw "package install did not add user package"
        }
        $listUser = & $exe list
        if (($listUser -join "`n") -notmatch [regex]::Escape($packageCliSource)) {
            throw "package list did not show user package"
        }
        $removeUser = & $exe remove $packageCliSource
        if (($removeUser -join "`n") -notmatch "Removed package from user settings") {
            throw "package remove did not remove user package"
        }

        New-Item -ItemType Directory -Force -Path (Join-Path $packageCliProject ".pi\prompts") | Out-Null
        $oldErrorActionPreference = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        $blockedLocalInstall = & $exe install $packageCliSource -l 2>&1
        $blockedLocalInstallExitCode = $LASTEXITCODE
        $ErrorActionPreference = $oldErrorActionPreference
        if ($blockedLocalInstallExitCode -eq 0) {
            throw "project package install succeeded without trust"
        }
        if (($blockedLocalInstall -join "`n") -notmatch "not trusted") {
            throw "project package install did not explain trust requirement"
        }
        $installProject = & $exe install $packageCliSource -l --approve
        if (($installProject -join "`n") -notmatch "Added package to project settings") {
            throw "package install -l --approve did not add project package"
        }
        $listProject = & $exe list --approve
        if (($listProject -join "`n") -notmatch "project\s+") {
            throw "package list --approve did not show project package scope"
        }
    } finally {
        Pop-Location
    }

    Write-Host "== npmCommand package smoke =="
    $fakeNpmLog = Join-Path $SmokeRoot "fake-npm.log"
    $fakeNpmScript = Join-Path $SmokeRoot "fake-npm.ps1"
    @'
param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Rest
)
if ($env:BBARIT_FAKE_NPM_LOG) {
    Add-Content -Path $env:BBARIT_FAKE_NPM_LOG -Value ($Rest -join "|")
}
exit 0
'@ | Set-Content -Path $fakeNpmScript -Encoding UTF8
    $env:BBARIT_FAKE_NPM_LOG = $fakeNpmLog
    $agentSettingsPath = Join-Path $env:PI_CODING_AGENT_DIR "settings.json"
    @{
        npmCommand = @("powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $fakeNpmScript)
    } | ConvertTo-Json -Depth 5 | Set-Content -Path $agentSettingsPath -Encoding UTF8
    $npmCommandProject = Join-Path $SmokeRoot "npm-command-project"
    New-Item -ItemType Directory -Force -Path $npmCommandProject | Out-Null
    Push-Location $npmCommandProject
    try {
        $customNpmInstall = & $exe install npm:fake-package
        if (($customNpmInstall -join "`n") -notmatch "Installed npm package") {
            throw "npmCommand npm package install did not report install"
        }
        $npmLogText = Get-Content -Path $fakeNpmLog -Raw
        if ($npmLogText -notmatch "install\|fake-package\|--prefix\|.*\|--legacy-peer-deps") {
            throw "npmCommand was not used for npm package install"
        }
        $customNpmRemove = & $exe remove npm:fake-package
        if (($customNpmRemove -join "`n") -notmatch "Removed package from user settings") {
            throw "npmCommand npm package remove did not report removal"
        }
        $npmRemoveLogText = Get-Content -Path $fakeNpmLog -Raw
        if ($npmRemoveLogText -notmatch "uninstall\|fake-package\|--prefix\|") {
            throw "npmCommand was not used for npm package uninstall"
        }
    } finally {
        Pop-Location
    }
    Clear-Content -Path $fakeNpmLog
    $customGitProject = Join-Path $SmokeRoot "npm-command-git-project"
    $customGitSource = Join-Path $SmokeRoot "npm-command-git-source"
    New-Item -ItemType Directory -Force -Path $customGitProject | Out-Null
    New-Item -ItemType Directory -Force -Path $customGitSource | Out-Null
    @{
        name = "npm-command-git-pi-package"
        dependencies = @{
            "fake-runtime-dependency" = "1.0.0"
        }
    } | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $customGitSource "package.json") -Encoding UTF8
    Invoke-NativeChecked "git" @("-C", $customGitSource, "init")
    Invoke-NativeChecked "git" @("-C", $customGitSource, "add", ".")
    Invoke-NativeChecked "git" @("-C", $customGitSource, "-c", "user.email=smoke@example.com", "-c", "user.name=Smoke", "commit", "-m", "initial")
    Push-Location $customGitProject
    try {
        $customGitInstall = & $exe install "git:$customGitSource"
        if (($customGitInstall -join "`n") -notmatch "Installed git package") {
            throw "npmCommand git package install did not report install"
        }
        $gitNpmLogText = Get-Content -Path $fakeNpmLog -Raw
        if ($gitNpmLogText.Trim() -ne "install") {
            throw "npmCommand git dependency install did not use plain install"
        }
    } finally {
        Pop-Location
    }
    "{}" | Set-Content -Path $agentSettingsPath -Encoding UTF8
    Remove-Item Env:\BBARIT_FAKE_NPM_LOG -ErrorAction SilentlyContinue

    Write-Host "== git package install smoke =="
    $gitPackageProject = Join-Path $SmokeRoot "git-package-project"
    $gitPackageSource = Join-Path $SmokeRoot "git-package-source"
    New-Item -ItemType Directory -Force -Path $gitPackageProject | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $gitPackageSource "extensions\git-ext") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $gitPackageSource "prompts") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $gitPackageSource "skills\git-skill") | Out-Null
    @{
        name = "git-pi-package"
        pi = @{
            extensions = @("./extensions")
            prompts = @("./prompts")
            skills = @("./skills")
        }
    } | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $gitPackageSource "package.json") -Encoding UTF8
    @{
        id = "git-pkg-extension"
        name = "Git Package Extension"
        commands = @{}
    } | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $gitPackageSource "extensions\git-ext\extension.json") -Encoding UTF8
    "---`ndescription: Git package prompt`n---`ngit prompt" | Set-Content -Path (Join-Path $gitPackageSource "prompts\git-prompt.md") -Encoding UTF8
    "---`nname: git-package-skill`ndescription: Git package skill`n---`ngit skill" | Set-Content -Path (Join-Path $gitPackageSource "skills\git-skill\SKILL.md") -Encoding UTF8
    Invoke-NativeChecked "git" @("-C", $gitPackageSource, "init")
    Invoke-NativeChecked "git" @("-C", $gitPackageSource, "add", ".")
    Invoke-NativeChecked "git" @("-C", $gitPackageSource, "-c", "user.email=smoke@example.com", "-c", "user.name=Smoke", "commit", "-m", "initial")
    Push-Location $gitPackageProject
    try {
        $gitSource = "git:$gitPackageSource"
        $gitInstall = & $exe install $gitSource
        if (($gitInstall -join "`n") -notmatch "Installed git package") {
            throw "git package install did not clone package"
        }
        $gitExtensions = & $exe --no-session --print /extensions
        if (($gitExtensions -join "`n") -notmatch "git-pkg-extension") {
            throw "installed git package extension did not load"
        }
        $gitPrompts = & $exe --no-session --print /prompts
        if (($gitPrompts -join "`n") -notmatch "git-prompt") {
            throw "installed git package prompt did not load"
        }
        $gitSkills = & $exe --no-session --print /skills
        if (($gitSkills -join "`n") -notmatch "git-package-skill") {
            throw "installed git package skill did not load"
        }
        "---`ndescription: Git package prompt updated`n---`ngit prompt updated" | Set-Content -Path (Join-Path $gitPackageSource "prompts\git-prompt-updated.md") -Encoding UTF8
        Invoke-NativeChecked "git" @("-C", $gitPackageSource, "add", ".")
        Invoke-NativeChecked "git" @("-C", $gitPackageSource, "-c", "user.email=smoke@example.com", "-c", "user.name=Smoke", "commit", "-m", "update prompt")
        $gitUpdate = & $exe update --extensions
        if (($gitUpdate -join "`n") -notmatch "Updated git package") {
            throw "git package update did not report update"
        }
        $gitUpdatedPrompts = & $exe --no-session --print /prompts
        if (($gitUpdatedPrompts -join "`n") -notmatch "git-prompt-updated") {
            throw "git package update did not reconcile new prompt"
        }
        $gitRemove = & $exe remove $gitSource
        if (($gitRemove -join "`n") -notmatch "Removed installed package") {
            throw "git package remove did not remove clone"
        }
    } finally {
        Pop-Location
    }

    Write-Host "== pinned git package smoke =="
    $pinnedGitProject = Join-Path $SmokeRoot "pinned-git-package-project"
    $pinnedGitSource = Join-Path $SmokeRoot "pinned-git-package-source"
    New-Item -ItemType Directory -Force -Path $pinnedGitProject | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $pinnedGitSource "prompts") | Out-Null
    @{
        name = "pinned-git-pi-package"
        pi = @{
            prompts = @("./prompts")
        }
    } | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $pinnedGitSource "package.json") -Encoding UTF8
    "---`ndescription: Pinned prompt v1`n---`npinned v1" | Set-Content -Path (Join-Path $pinnedGitSource "prompts\pinned-v1.md") -Encoding UTF8
    Invoke-NativeChecked "git" @("-C", $pinnedGitSource, "init")
    Invoke-NativeChecked "git" @("-C", $pinnedGitSource, "add", ".")
    Invoke-NativeChecked "git" @("-C", $pinnedGitSource, "-c", "user.email=smoke@example.com", "-c", "user.name=Smoke", "commit", "-m", "v1")
    Invoke-NativeChecked "git" @("-C", $pinnedGitSource, "tag", "v1")
    Push-Location $pinnedGitProject
    try {
        $pinnedGitSpec = "git:$pinnedGitSource@v1"
        $pinnedInstall = & $exe install $pinnedGitSpec
        if (($pinnedInstall -join "`n") -notmatch "Installed git package") {
            throw "pinned git package install did not clone package"
        }
        $pinnedPrompts = & $exe --no-session --print /prompts
        if (($pinnedPrompts -join "`n") -notmatch "pinned-v1") {
            throw "pinned git package did not expose v1 prompt"
        }
        "---`ndescription: Pinned prompt v2`n---`npinned v2" | Set-Content -Path (Join-Path $pinnedGitSource "prompts\pinned-v2.md") -Encoding UTF8
        Invoke-NativeChecked "git" @("-C", $pinnedGitSource, "add", ".")
        Invoke-NativeChecked "git" @("-C", $pinnedGitSource, "-c", "user.email=smoke@example.com", "-c", "user.name=Smoke", "commit", "-m", "v2")
        $pinnedUpdate = & $exe update --all
        if (($pinnedUpdate -join "`n") -notmatch "Updated git package") {
            throw "pinned git package update did not reconcile package"
        }
        $pinnedUpdatedPrompts = & $exe --no-session --print /prompts
        if (($pinnedUpdatedPrompts -join "`n") -match "pinned-v2") {
            throw "pinned git package moved away from configured ref"
        }
    } finally {
        Pop-Location
    }

    Write-Host "== session smoke =="
    $sessionDir = Join-Path $SmokeRoot "sessions"
    & $exe --session-dir $sessionDir --print "/name Smoke Session"
    $sessionList = & $exe --session-dir $sessionDir --print /sessions
    $sessionList
    & $exe --session-dir $sessionDir --print /resume
    $shareOutput = & $exe --session-dir $sessionDir --continue --print /share
    $shareOutput
    if ($shareOutput -notmatch "Shareable history: (.+\.html)") {
        throw "share command did not report an HTML path"
    }
    $sharePath = $Matches[1]
    if (-not (Test-Path $sharePath)) {
        throw "share command did not create $sharePath"
    }
    if (-not (Select-String -Path $sharePath -Pattern "bbarit Session Export" -Quiet)) {
        throw "share HTML did not contain expected export marker"
    }
    $sessionFile = (($sessionList | Select-Object -First 1) -split "`t")[-1]
    $cliExportPath = Join-Path $SmokeRoot "cli-export.html"
    $cliExport = & $exe --export $sessionFile $cliExportPath
    if (($cliExport -join "`n") -notmatch "Exported HTML") {
        throw "--export did not report HTML export"
    }
    if (-not (Test-Path $cliExportPath)) {
        throw "--export did not create $cliExportPath"
    }
    if (-not (Select-String -Path $cliExportPath -Pattern "bbarit Session Export" -Quiet)) {
        throw "--export HTML did not contain expected marker"
    }
    $defaultSlashExport = & $exe --session-dir $sessionDir --session $sessionFile --print /export
    $defaultSlashExportText = $defaultSlashExport -join "`n"
    if ($defaultSlashExportText -notmatch "Exported HTML (.+\.html)") {
        throw "/export without path did not report an HTML export"
    }
    $defaultSlashExportPath = $Matches[1]
    if (-not (Test-Path $defaultSlashExportPath)) {
        throw "/export without path did not create $defaultSlashExportPath"
    }
    if (-not (Select-String -Path $defaultSlashExportPath -Pattern "bbarit Session Export" -Quiet)) {
        throw "/export default HTML did not contain expected marker"
    }
    $importSourcePath = Join-Path $SmokeRoot "import-source.jsonl"
    $jsonlExport = & $exe --session-dir $sessionDir --session $sessionFile --print "/export $importSourcePath"
    if (($jsonlExport -join "`n") -notmatch "Exported JSONL") {
        throw "/export .jsonl did not report JSONL export"
    }
    if (-not (Test-Path $importSourcePath)) {
        throw "/export .jsonl did not create $importSourcePath"
    }
    if (-not (Select-String -Path $importSourcePath -Pattern '"type":"session"' -Quiet)) {
        throw "/export .jsonl did not write a session JSONL header"
    }
    $importOutput = & $exe --session-dir $sessionDir --print "/import $importSourcePath"
    if (($importOutput -join "`n") -notmatch "Imported session:") {
        throw "/import did not report imported session"
    }
    $importedSessionPath = Join-Path $sessionDir "import-source.jsonl"
    if (-not (Test-Path $importedSessionPath)) {
        throw "/import did not copy JSONL into $importedSessionPath"
    }
    $importedSession = & $exe --session-dir $sessionDir --session $importedSessionPath --print /session
    if (($importedSession -join "`n") -notmatch "Name: Smoke Session") {
        throw "/import did not preserve imported session data"
    }
    $sessionId = (($sessionList | Select-Object -First 1) -split "`t")[0]
    $fixedSessionId = "smoke-fixed-session"
    & $exe --session-dir $sessionDir --session-id $fixedSessionId --print "/name Fixed Session"
    $fixedSession = & $exe --session-dir $sessionDir --session-id $fixedSessionId --print /session
    if (($fixedSession -join "`n") -notmatch "Session $fixedSessionId") {
        throw "--session-id did not open exact session id"
    }
    if (($fixedSession -join "`n") -notmatch "Name: Fixed Session") {
        throw "--session-id did not reopen existing session data"
    }
    $fixedSessionPath = Join-Path $sessionDir "$fixedSessionId.jsonl"
    if (-not (Test-Path $fixedSessionPath)) {
        throw "--session-id did not create $fixedSessionPath"
    }
    $forkOutput = & $exe --session-dir $sessionDir --fork $sessionFile --print /session
    $forkText = $forkOutput -join "`n"
    if ($forkText -notmatch "Session ([^\r\n]+)") {
        throw "--fork did not report a session"
    }
    $forkSessionId = $Matches[1]
    if ($forkSessionId -eq $sessionId) {
        throw "--fork reused source session id"
    }
    $forkSessionPath = Join-Path $sessionDir "$forkSessionId.jsonl"
    if (-not (Test-Path $forkSessionPath)) {
        throw "--fork did not create $forkSessionPath"
    }
    if (-not (Select-String -Path $forkSessionPath -Pattern "parentSession" -Quiet)) {
        throw "--fork did not record parentSession"
    }

    Write-Host "== tool smoke =="
    $toolDir = Join-Path $SmokeRoot "tool"
    New-Item -ItemType Directory -Force -Path $toolDir | Out-Null
    @(
        'fn main() {',
        '    println!("alpha");',
        '    println!("beta");',
        '}'
    ) -join "`n" | Set-Content -Path (Join-Path $toolDir "sample.rs") -Encoding UTF8
    & $exe --no-session --print "/read $toolDir\sample.rs"
    & $exe --no-session --print "/grep println $toolDir"
    & $exe --no-session --print "/find sample $toolDir"
    $bangSessionDir = Join-Path $SmokeRoot "bang-sessions"
    $bangOutput = & $exe --approve --session-dir $bangSessionDir --session-id bang-visible --print "!Write-Output visible-bash"
    if (($bangOutput -join "`n") -notmatch "visible-bash") {
        throw "!command did not execute bash"
    }
    $bangSession = & $exe --session-dir $bangSessionDir --session-id bang-visible --print /session
    if (($bangSession -join "`n") -notmatch "Messages:\s+1") {
        throw "!command did not persist output into context"
    }
    $doubleBangOutput = & $exe --approve --session-dir $bangSessionDir --session-id bang-hidden --print "!!Write-Output hidden-bash"
    if (($doubleBangOutput -join "`n") -notmatch "hidden-bash") {
        throw "!!command did not execute bash"
    }
    $doubleBangSession = & $exe --session-dir $bangSessionDir --session-id bang-hidden --print /session
    if (($doubleBangSession -join "`n") -notmatch "Messages:\s+0") {
        throw "!!command persisted output into context"
    }

    Write-Host "== extension settings smoke =="
    $extensionProject = Join-Path $SmokeRoot "extension-settings-project"
    $externalExtension = Join-Path $SmokeRoot "external-extension"
    $tsExtension = Join-Path $SmokeRoot "ts-extension"
    New-Item -ItemType Directory -Force -Path (Join-Path $extensionProject ".pi") | Out-Null
    New-Item -ItemType Directory -Force -Path $externalExtension | Out-Null
    New-Item -ItemType Directory -Force -Path $tsExtension | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $externalExtension "themes") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $externalExtension "dynamic-prompts") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $externalExtension "dynamic-skills\dyn-skill") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $externalExtension "dynamic-themes") | Out-Null
    @{
        extensions = @($externalExtension, $tsExtension)
    } | ConvertTo-Json | Set-Content -Path (Join-Path $extensionProject ".pi\settings.json") -Encoding UTF8
    @{
        id = "external-smoke"
        name = "External Smoke"
        description = "Loaded from settings.extensions"
        commands = @{
            hello = @{
                description = "Smoke command"
                command = "Write-Output extension-ok"
            }
        }
    } | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $externalExtension "extension.json") -Encoding UTF8
    @(
        "import fs from 'node:fs';",
        "export default function(pi) {",
        "  pi.registerCommand('runtimehello', {",
        "    description: 'Runtime smoke command',",
        "    handler: async (args, ctx) => {",
        "      ctx.notify('runtime-command-ok ' + args);",
        "      ctx.ui.notify('ui-notify-ok', 'warning');",
        "      ctx.ui.setStatus('runtime-status', 'ready');",
        "      ctx.ui.setWidget('runtime-widget', ['widget-line'], { placement: 'belowEditor' });",
        "      ctx.ui.setTitle('runtime-title');",
        "      ctx.ui.setWorkingMessage('runtime-working');",
        "      ctx.ui.setWorkingVisible(false);",
        "      ctx.ui.setWorkingIndicator({ frames: ['.'] });",
        "      ctx.ui.setEditorText('prefill');",
        "      ctx.ui.pasteToEditor('-paste');",
        "      ctx.ui.setToolsExpanded(true);",
        "      return { args };",
        "    }",
        "  });",
        "  pi.registerTool({",
        "    name: 'runtime_tool_smoke',",
        "    label: 'Runtime Tool Smoke',",
        "    description: 'Runtime extension tool smoke',",
        "    promptSnippet: 'runtime_tool_smoke: use for smoke metadata checks',",
        "    promptGuidelines: ['prefer runtime_tool_smoke for prompt metadata smoke'],",
        "    parameters: { type: 'object', properties: { value: { type: 'string' } }, required: ['value'] },",
        "    execute: async (toolCallId, params, signal, onUpdate, ctx) => {",
        "      ctx.notify('tool-notify ' + toolCallId);",
        "      return { content: [{ type: 'text', text: 'tool-ok ' + params.value }] };",
        "    }",
        "  });",
        "  pi.registerTool({",
        "    name: 'read',",
        "    label: 'Read Override Smoke',",
        "    description: 'Extension read override smoke',",
        "    parameters: { type: 'object', properties: { path: { type: 'string' } } },",
        "    execute: async (toolCallId, params) => {",
        "      return { content: [{ type: 'text', text: 'override-read-ok ' + (params.path || '') }] };",
        "    }",
        "  });",
        "  pi.registerTool({",
        "    name: 'terminating_tool_smoke',",
        "    label: 'Terminating Tool Smoke',",
        "    description: 'Terminating extension tool smoke',",
        "    promptSnippet: 'terminating_tool_smoke: final tool result smoke',",
        "    promptGuidelines: ['Use terminating_tool_smoke only as a final tool action.'],",
        "    parameters: { type: 'object', properties: { value: { type: 'string' } }, required: ['value'] },",
        "    execute: async (toolCallId, params) => {",
        "      return { content: [{ type: 'text', text: 'terminate-ok ' + params.value }], terminate: true };",
        "    }",
        "  });",
        "  pi.registerShortcut('ctrl+shift+y', {",
        "    description: 'Runtime shortcut smoke',",
        "    handler: async (ctx) => {",
        "      ctx.notify('shortcut-ok');",
        "      return 'shortcut-result';",
        "    }",
        "  });",
        "  pi.registerProvider('runtime-provider-smoke', {",
        "    name: 'Runtime Provider Smoke',",
        "    baseUrl: process.env.RUNTIME_PROVIDER_SMOKE_BASE_URL || 'http://127.0.0.1:19999/v1',",
        "    apiKey: '`$RUNTIME_PROVIDER_SMOKE_KEY',",
        "    authHeader: true,",
        "    headers: { 'x-runtime-provider': 'provider-header-ok' },",
        "    api: 'openai-completions',",
        "    models: [{",
        "      id: 'runtime-model-smoke',",
        "      name: 'Runtime Model Smoke',",
        "      reasoning: true,",
        "      contextWindow: 12345,",
        "      maxTokens: 678",
        "    }]",
        "  });",
        "  pi.registerProvider('extension-stream-smoke', {",
        "    name: 'Extension Stream Smoke',",
        "    api: 'extension-stream-smoke-api',",
        "    models: [{",
        "      id: 'extension-stream-model',",
        "      name: 'Extension Stream Model',",
        "      reasoning: true,",
        "      contextWindow: 4096,",
        "      maxTokens: 512",
        "    }],",
        "    streamSimple: async (model, context, options) => ({",
        "      text: 'extension-stream-ok ' + model.id + ' messages:' + ((context.messages || []).length) + ' tools:' + Array.isArray(options.tools),",
        "      usage: { input: 2, output: 3, total: 5 }",
        "    })",
        "  });",
        "  pi.unregisterProvider('ant-ling');",
        "  pi.on('input', (event, ctx) => {",
        "    if (event.text === 'input-hook-smoke') {",
        "      ctx.notify('input-hook-ok');",
        "      return { action: 'handled' };",
        "    }",
        "    return { action: 'continue' };",
        "  });",
        "  pi.on('user_bash', (event) => 'bash-hook-ok ' + event.command);",
        "  pi.on('session_before_switch', (event) => {",
        "    if (event.targetSessionFile === 'cancel-me') return { cancel: true };",
        "    return 'before-switch ' + event.reason;",
        "  });",
        "  pi.on('session_shutdown', (event) => 'shutdown ' + event.reason);",
        "  pi.on('session_start', (event) => 'session-start ' + event.reason);",
        "  pi.on('resources_discover', () => ({",
        "    promptPaths: ['./dynamic-prompts'],",
        "    skillPaths: ['./dynamic-skills'],",
        "    themePaths: ['./dynamic-themes']",
        "  }));",
        "  pi.on('before_provider_request', (event) => ({ ...event.payload, metadata: { providerHook: 'before-ok' } }));",
        "  pi.on('context', (event) => ({ messages: [...event.messages, { role: 'user', content: 'context-hook-message' }] }));",
        "  pi.on('tool_result', (event) => {",
        "    if (event.toolName === 'terminating_tool_smoke') {",
        "      const text = event.content.map((item) => item.text || '').join(' ');",
        "      return { content: [{ type: 'text', text: text + ' tool-result-hook-ok' }], isError: false };",
        "    }",
        "  });",
        "  pi.on('after_provider_response', (event) => {",
        "    if (process.env.PROVIDER_HOOK_MARKER) fs.writeFileSync(process.env.PROVIDER_HOOK_MARKER, 'status ' + event.status);",
        "  });",
        "  pi.on('session_start', (payload) => ({ smoke: 'runtime-ok', reason: payload.reason }));",
        "  pi.on('tool_call', (event) => {",
        "    if (event.toolName === 'terminating_tool_smoke') {",
        "      event.input.value = 'mutated';",
        "    }",
        "    if (event.toolName === 'runtime_tool_smoke' && event.input.value === 'block-me') {",
        "      return { block: true, reason: 'blocked by hook' };",
        "    }",
        "  });",
        "}"
    ) -join "`n" | Set-Content -Path (Join-Path $externalExtension "index.mjs") -Encoding UTF8
    @(
        "import type { ExtensionAPI } from '@earendil-works/pi-coding-agent';",
        "type Args = string;",
        "export default function(pi: ExtensionAPI) {",
        "  pi.registerCommand('tshello', {",
        "    description: 'TypeScript runtime smoke command',",
        "    handler: async (args: Args, ctx: any) => {",
        "      ctx.ui.notify('ts-ui-ok', 'info');",
        "      return 'ts-ok ' + args;",
        "    }",
        "  });",
        "}"
    ) -join "`n" | Set-Content -Path (Join-Path $tsExtension "index.ts") -Encoding UTF8
    "---`ndescription: Dynamic extension prompt`n---`nDynamic prompt body" | Set-Content -Path (Join-Path $externalExtension "dynamic-prompts\dyn-prompt.md") -Encoding UTF8
    "---`nname: dyn-skill`ndescription: Dynamic extension skill`n---`nDynamic skill body" | Set-Content -Path (Join-Path $externalExtension "dynamic-skills\dyn-skill\SKILL.md") -Encoding UTF8
    @{
        name = "dyn-theme"
        colors = @{
            accent = "#123456"
            text = "#ffffff"
        }
    } | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $externalExtension "dynamic-themes\dyn-theme.json") -Encoding UTF8
    @{
        name = "ext-theme"
        colors = @{
            accent = "#abcdef"
            border = "#abcdef"
            muted = "#888888"
            text = "#ffffff"
        }
    } | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $externalExtension "themes\ext-theme.json") -Encoding UTF8
    Push-Location $extensionProject
    try {
        $extensionList = & $exe --approve --no-session --print /extensions
        $extensionList
        if (($extensionList -join "`n") -notmatch "external-smoke") {
            throw "settings.extensions did not load external extension"
        }
        $extensionThemes = & $exe --approve --no-session --print /themes
        if (($extensionThemes -join "`n") -notmatch "ext-theme") {
            throw "settings.extensions theme did not load"
        }
        $extensionPrompts = & $exe --approve --no-session --print /prompts
        if (($extensionPrompts -join "`n") -notmatch "dyn-prompt") {
            throw "resources_discover prompt path did not load"
        }
        $extensionSkills = & $exe --approve --no-session --print /skills
        if (($extensionSkills -join "`n") -notmatch "dyn-skill") {
            throw "resources_discover skill path did not load"
        }
        $extensionThemes = & $exe --approve --no-session --print /themes
        if (($extensionThemes -join "`n") -notmatch "dyn-theme") {
            throw "resources_discover theme path did not load"
        }
        $extensionHotkeys = & $exe --approve --no-session --print /hotkeys
        if (($extensionHotkeys -join "`n") -notmatch "ctrl\+shift\+y" -or ($extensionHotkeys -join "`n") -notmatch "Runtime shortcut smoke") {
            throw "registerShortcut metadata did not load into hotkeys"
        }
        $extensionProviders = & $exe --approve --no-session --print /providers
        if (($extensionProviders -join "`n") -notmatch "runtime-provider-smoke" -or ($extensionProviders -join "`n") -notmatch "Runtime Provider Smoke") {
            throw "registerProvider did not add extension provider"
        }
        if (($extensionProviders -join "`n") -notmatch "extension-stream-smoke" -or ($extensionProviders -join "`n") -notmatch "Extension Stream Smoke") {
            throw "registerProvider streamSimple provider did not load"
        }
        if (($extensionProviders -join "`n") -match "ant-ling") {
            throw "unregisterProvider did not remove built-in provider"
        }
        $extensionModels = & $exe --approve --no-session --print "/models runtime-provider-smoke"
        if (($extensionModels -join "`n") -notmatch "runtime-model-smoke" -or ($extensionModels -join "`n") -notmatch "Runtime Model Smoke") {
            throw "registerProvider did not add extension model"
        }
        $unregisteredModels = & $exe --approve --no-session --print "/models ant-ling"
        if (($unregisteredModels -join "`n") -match "Ring-") {
            throw "unregisterProvider did not remove provider models"
        }
        $streamModels = & $exe --approve --no-session --print "/models extension-stream-smoke"
        if (($streamModels -join "`n") -notmatch "extension-stream-model" -or ($streamModels -join "`n") -notmatch "Extension Stream Model") {
            throw "registerProvider streamSimple model did not load"
        }
        $extensionModelSwitch = & $exe --approve --no-session --print "/model runtime-provider-smoke/runtime-model-smoke"
        if (($extensionModelSwitch -join "`n") -notmatch "Model set: runtime-provider-smoke/runtime-model-smoke") {
            throw "registerProvider extension model did not resolve"
        }
        $streamOutput = & $exe --approve --no-session --provider extension-stream-smoke --model extension-stream-model --print "extension stream smoke"
        if (($streamOutput -join "`n") -notmatch "extension-stream-ok extension-stream-model" -or ($streamOutput -join "`n") -notmatch "tools:True|tools:true") {
            throw "registerProvider streamSimple provider did not handle completion"
        }
        $extensionShortcut = & $exe --approve --no-session --print "/shortcut ctrl+shift+y"
        if (($extensionShortcut -join "`n") -notmatch "shortcut-ok" -or ($extensionShortcut -join "`n") -notmatch "shortcut-result") {
            throw "registerShortcut handler did not execute"
        }
        $extensionDetail = & $exe --approve --no-session --print "/extension external-smoke"
        if (($extensionDetail -join "`n") -notmatch "session_start" -or ($extensionDetail -join "`n") -notmatch "tool_call") {
            throw "settings.extensions hook metadata did not load"
        }
        if (($extensionDetail -join "`n") -notmatch "index.mjs") {
            throw "settings.extensions entry metadata did not load"
        }
        if (($extensionDetail -join "`n") -notmatch "runtimehello") {
            throw "settings.extensions runtime command metadata did not load"
        }
        $runtimeCommand = & $exe --approve --no-session --print "/x runtimehello smoke-args"
        if (($runtimeCommand -join "`n") -notmatch "runtime-command-ok smoke-args") {
            throw "settings.extensions runtime command did not execute"
        }
        if (($runtimeCommand -join "`n") -notmatch "ui-notify-ok" -or ($runtimeCommand -join "`n") -notmatch "ui\.setStatus runtime-status: ready" -or ($runtimeCommand -join "`n") -notmatch "ui\.setWidget runtime-widget" -or ($runtimeCommand -join "`n") -notmatch "ui\.setTitle runtime-title" -or ($runtimeCommand -join "`n") -notmatch "ui\.setEditorText prefill" -or ($runtimeCommand -join "`n") -notmatch "ui\.pasteToEditor -paste" -or ($runtimeCommand -join "`n") -notmatch "ui\.setToolsExpanded true") {
            throw "settings.extensions ctx.ui methods did not execute"
        }
        $tsRuntimeCommand = & $exe --approve --no-session --print "/x tshello smoke-ts"
        if (($tsRuntimeCommand -join "`n") -notmatch "ts-ok smoke-ts" -or ($tsRuntimeCommand -join "`n") -notmatch "ts-ui-ok") {
            throw "settings.extensions TypeScript runtime command did not execute"
        }
        $bashHook = & $exe --approve --no-session --print "!!Write-Output bash-hook-shell"
        if (($bashHook -join "`n") -notmatch "bash-hook-ok Write-Output bash-hook-shell") {
            throw "settings.extensions user_bash hook did not execute automatically"
        }
        $inputHook = & $exe --approve --no-session --print "input-hook-smoke"
        if (($inputHook -join "`n") -notmatch "input-hook-ok" -or ($inputHook -join "`n") -notmatch "Input handled by extension") {
            throw "settings.extensions input hook did not handle input automatically"
        }
        $newHook = & $exe --approve --no-session --print "/new"
        if (($newHook -join "`n") -notmatch "before-switch new" -or ($newHook -join "`n") -notmatch "shutdown new" -or ($newHook -join "`n") -notmatch "session-start new") {
            throw "settings.extensions session lifecycle hooks did not run for /new"
        }
        $resumeCancel = & $exe --approve --no-session --print "/resume cancel-me"
        if (($resumeCancel -join "`n") -notmatch "Session switch cancelled by extension") {
            throw "settings.extensions session_before_switch did not cancel /resume"
        }
        $extensionRpcInput = @(
            '{"id":"extensions","type":"get_extensions"}',
            '{"id":"runtime","type":"run_extension_hook","extensionId":"external-smoke","event":"session_start","payload":{"reason":"smoke"}}',
            '{"id":"tools","type":"get_available_tools"}',
            '{"id":"toolrun","type":"run_extension_tool","toolName":"runtime_tool_smoke","toolCallId":"tool-call-smoke","arguments":{"value":"smoke-value"}}',
            '{"id":"toolrun-read","type":"run_extension_tool","toolName":"read","toolCallId":"tool-call-read","arguments":{"path":"override.txt"}}',
            '{"id":"shortcutrun","type":"run_extension_shortcut","shortcut":"ctrl+shift+y"}'
        )
        $extensionRpcEvents = $extensionRpcInput | & $exe --mode rpc --no-session --approve | ForEach-Object { $_ | ConvertFrom-Json }
        $extensionRpc = $extensionRpcEvents | Where-Object { $_.type -eq "response" -and $_.command -eq "get_extensions" } | Select-Object -First 1
        if (-not $extensionRpc.success -or (($extensionRpc.data.extensions | ConvertTo-Json -Depth 10) -notmatch "session_start") -or (($extensionRpc.data.extensions | ConvertTo-Json -Depth 10) -notmatch "index.mjs")) {
            throw "rpc get_extensions did not return hook metadata"
        }
        $runtimeRpc = $extensionRpcEvents | Where-Object { $_.type -eq "response" -and $_.command -eq "run_extension_hook" } | Select-Object -First 1
        if (-not $runtimeRpc.success -or (($runtimeRpc.data | ConvertTo-Json -Depth 10) -notmatch "runtime-ok")) {
            throw "rpc run_extension_hook did not execute extension runtime"
        }
        $toolsRpc = $extensionRpcEvents | Where-Object { $_.type -eq "response" -and $_.command -eq "get_available_tools" } | Select-Object -First 1
        if (-not $toolsRpc.success -or (($toolsRpc.data | ConvertTo-Json -Depth 20) -notmatch "runtime_tool_smoke")) {
            throw "rpc get_available_tools did not include extension tool"
        }
        if (($toolsRpc.data | ConvertTo-Json -Depth 20) -notmatch "runtime_tool_smoke: use for smoke metadata checks" -or ($toolsRpc.data | ConvertTo-Json -Depth 20) -notmatch "prefer runtime_tool_smoke for prompt metadata smoke") {
            throw "rpc get_available_tools did not include extension tool prompt metadata"
        }
        if (($toolsRpc.data | ConvertTo-Json -Depth 20) -notmatch "Extension read override smoke") {
            throw "rpc get_available_tools did not let extension read override builtin read"
        }
        $toolRunRpc = $extensionRpcEvents | Where-Object { $_.type -eq "response" -and $_.command -eq "run_extension_tool" } | Select-Object -First 1
        if (-not $toolRunRpc.success -or (($toolRunRpc.data | ConvertTo-Json -Depth 20) -notmatch "tool-ok smoke-value") -or (($toolRunRpc.data | ConvertTo-Json -Depth 20) -notmatch "tool-notify tool-call-smoke")) {
            throw "rpc run_extension_tool did not execute extension tool"
        }
        $toolRunReadRpc = $extensionRpcEvents | Where-Object { $_.type -eq "response" -and $_.id -eq "toolrun-read" } | Select-Object -First 1
        if (-not $toolRunReadRpc.success -or (($toolRunReadRpc.data | ConvertTo-Json -Depth 20) -notmatch "override-read-ok override.txt")) {
            throw "rpc run_extension_tool did not execute extension read override"
        }
        $noBuiltinToolsRpc = @('{"id":"tools","type":"get_available_tools"}') | & $exe --mode rpc --no-builtin-tools --no-session --approve | ForEach-Object { $_ | ConvertFrom-Json }
        $noBuiltinToolsResponse = $noBuiltinToolsRpc | Where-Object { $_.type -eq "response" -and $_.command -eq "get_available_tools" } | Select-Object -First 1
        $noBuiltinToolsJson = $noBuiltinToolsResponse.data | ConvertTo-Json -Depth 20
        if (-not $noBuiltinToolsResponse.success -or $noBuiltinToolsJson -notmatch "runtime_tool_smoke" -or $noBuiltinToolsJson -notmatch "Extension read override smoke") {
            throw "--no-builtin-tools did not keep extension tools active"
        }
        if ($noBuiltinToolsJson -match "Read a text file") {
            throw "--no-builtin-tools leaked builtin read tool"
        }
        $shortcutRunRpc = $extensionRpcEvents | Where-Object { $_.type -eq "response" -and $_.command -eq "run_extension_shortcut" } | Select-Object -First 1
        if (-not $shortcutRunRpc.success -or (($shortcutRunRpc.data | ConvertTo-Json -Depth 20) -notmatch "shortcut-ok") -or (($shortcutRunRpc.data | ConvertTo-Json -Depth 20) -notmatch "shortcut-result")) {
            throw "rpc run_extension_shortcut did not execute extension shortcut"
        }

        Write-Host "== provider hook smoke =="
        $port = 19080 + (Get-Random -Minimum 0 -Maximum 1000)
        $requestLog = Join-Path $SmokeRoot "provider-hook-request.json"
        $headerLog = Join-Path $SmokeRoot "provider-hook-headers.json"
        $readyFile = Join-Path $SmokeRoot "provider-hook-ready.txt"
        $providerMarker = Join-Path $SmokeRoot "provider-hook-after.txt"
        Remove-Item $requestLog, $headerLog, $readyFile, $providerMarker -ErrorAction SilentlyContinue
        @{
            providers = @{
                "mock-hook" = @{
                    name = "Mock Hook"
                    api = "openai-completions"
                    baseUrl = "http://127.0.0.1:$port/v1"
                    apiKey = "dummy"
                    models = @(@{
                        id = "mock-model"
                        name = "Mock Model"
                        api = "openai-completions"
                        maxTokens = 128
                    })
                }
            }
        } | ConvertTo-Json -Depth 8 | Set-Content -Path (Join-Path $extensionProject ".pi\models.json") -Encoding UTF8
        $serverJob = Start-Job -ArgumentList $port, $requestLog, $headerLog, $readyFile -ScriptBlock {
            param($Port, $RequestLog, $HeaderLog, $ReadyFile)
            $listener = [System.Net.HttpListener]::new()
            $listener.Prefixes.Add("http://127.0.0.1:$Port/")
            try {
                $listener.Start()
                Set-Content -Path $ReadyFile -Value "ready" -Encoding UTF8
                $ctx = $listener.GetContext()
                $reader = [System.IO.StreamReader]::new($ctx.Request.InputStream, $ctx.Request.ContentEncoding)
                $body = $reader.ReadToEnd()
                $reader.Close()
                Set-Content -Path $RequestLog -Value $body -Encoding UTF8
                $headers = @{}
                foreach ($key in $ctx.Request.Headers.AllKeys) {
                    $headers[$key] = $ctx.Request.Headers[$key]
                }
                $headers | ConvertTo-Json -Depth 5 | Set-Content -Path $HeaderLog -Encoding UTF8
                $json = '{"id":"mock","choices":[{"message":{"role":"assistant","content":"provider-hook-response"}}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}'
                $bytes = [System.Text.Encoding]::UTF8.GetBytes($json)
                $ctx.Response.StatusCode = 200
                $ctx.Response.ContentType = "application/json"
                $ctx.Response.ContentEncoding = [System.Text.Encoding]::UTF8
                $ctx.Response.ContentLength64 = $bytes.Length
                $ctx.Response.KeepAlive = $false
                $ctx.Response.OutputStream.Write($bytes, 0, $bytes.Length)
                $ctx.Response.OutputStream.Flush()
                $ctx.Response.OutputStream.Close()
            } finally {
                if ($listener.IsListening) {
                    $listener.Stop()
                }
                $listener.Close()
            }
        }
        try {
            for ($i = 0; $i -lt 80 -and -not (Test-Path $readyFile); $i++) {
                Start-Sleep -Milliseconds 50
            }
            if (-not (Test-Path $readyFile)) {
                throw "provider hook mock server did not start"
            }
            $oldMarker = $env:PROVIDER_HOOK_MARKER
            $oldRuntimeBase = $env:RUNTIME_PROVIDER_SMOKE_BASE_URL
            $oldRuntimeKey = $env:RUNTIME_PROVIDER_SMOKE_KEY
            $env:PROVIDER_HOOK_MARKER = $providerMarker
            $env:RUNTIME_PROVIDER_SMOKE_BASE_URL = "http://127.0.0.1:$port/v1"
            $env:RUNTIME_PROVIDER_SMOKE_KEY = "runtime-provider-key"
            try {
                $providerHookOutput = & $exe --approve --no-session --provider runtime-provider-smoke --model runtime-model-smoke --print "provider hook smoke"
            } finally {
                $env:PROVIDER_HOOK_MARKER = $oldMarker
                $env:RUNTIME_PROVIDER_SMOKE_BASE_URL = $oldRuntimeBase
                $env:RUNTIME_PROVIDER_SMOKE_KEY = $oldRuntimeKey
            }
            if (($providerHookOutput -join "`n") -notmatch "provider-hook-response") {
                throw "provider hook mock completion did not return expected response"
            }
            if ((Get-Content $requestLog -Raw) -notmatch '"providerHook"\s*:\s*"before-ok"') {
                throw "before_provider_request did not transform provider payload"
            }
            if ((Get-Content $requestLog -Raw) -notmatch "context-hook-message") {
                throw "context hook did not transform provider messages"
            }
            if ((Get-Content $requestLog -Raw) -notmatch "runtime_tool_smoke: use for smoke metadata checks" -or (Get-Content $requestLog -Raw) -notmatch "prefer runtime_tool_smoke for prompt metadata smoke") {
                throw "extension tool prompt metadata did not reach provider system prompt"
            }
            $headerText = Get-Content $headerLog -Raw
            if ($headerText -notmatch '"x-runtime-provider"\s*:\s*"provider-header-ok"') {
                throw "extension registerProvider headers were not sent"
            }
            if ($headerText -notmatch '"Authorization"\s*:\s*"Bearer runtime-provider-key"') {
                throw "extension registerProvider authHeader did not send bearer auth"
            }
            if (-not (Test-Path $providerMarker) -or ((Get-Content $providerMarker -Raw) -notmatch "status 200")) {
                throw "after_provider_response did not observe provider response"
            }
        } finally {
            Stop-Job $serverJob -ErrorAction SilentlyContinue | Out-Null
            Remove-Job $serverJob -Force -ErrorAction SilentlyContinue | Out-Null
        }

        Write-Host "== terminating tool smoke =="
        $termPort = 20180 + (Get-Random -Minimum 0 -Maximum 1000)
        $termRequestLog = Join-Path $SmokeRoot "terminating-tool-request.json"
        $termReadyFile = Join-Path $SmokeRoot "terminating-tool-ready.txt"
        Remove-Item $termRequestLog, $termReadyFile -ErrorAction SilentlyContinue
        $termServerJob = Start-Job -ArgumentList $termPort, $termRequestLog, $termReadyFile -ScriptBlock {
            param($Port, $RequestLog, $ReadyFile)
            $listener = [System.Net.HttpListener]::new()
            $listener.Prefixes.Add("http://127.0.0.1:$Port/")
            try {
                $listener.Start()
                Set-Content -Path $ReadyFile -Value "ready" -Encoding UTF8
                $ctx = $listener.GetContext()
                $reader = [System.IO.StreamReader]::new($ctx.Request.InputStream, $ctx.Request.ContentEncoding)
                $body = $reader.ReadToEnd()
                $reader.Close()
                Set-Content -Path $RequestLog -Value $body -Encoding UTF8
                $json = '{"id":"mock-term","choices":[{"message":{"role":"assistant","content":"","tool_calls":[{"id":"term-call-1","type":"function","function":{"name":"terminating_tool_smoke","arguments":"{\"value\":\"done\"}"}}]}}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}'
                $bytes = [System.Text.Encoding]::UTF8.GetBytes($json)
                $ctx.Response.StatusCode = 200
                $ctx.Response.ContentType = "application/json"
                $ctx.Response.ContentEncoding = [System.Text.Encoding]::UTF8
                $ctx.Response.ContentLength64 = $bytes.Length
                $ctx.Response.KeepAlive = $false
                $ctx.Response.OutputStream.Write($bytes, 0, $bytes.Length)
                $ctx.Response.OutputStream.Flush()
                $ctx.Response.OutputStream.Close()
            } finally {
                if ($listener.IsListening) {
                    $listener.Stop()
                }
                $listener.Close()
            }
        }
        try {
            for ($i = 0; $i -lt 80 -and -not (Test-Path $termReadyFile); $i++) {
                Start-Sleep -Milliseconds 50
            }
            if (-not (Test-Path $termReadyFile)) {
                throw "terminating tool mock server did not start"
            }
            $oldRuntimeBase = $env:RUNTIME_PROVIDER_SMOKE_BASE_URL
            $oldRuntimeKey = $env:RUNTIME_PROVIDER_SMOKE_KEY
            $env:RUNTIME_PROVIDER_SMOKE_BASE_URL = "http://127.0.0.1:$termPort/v1"
            $env:RUNTIME_PROVIDER_SMOKE_KEY = "runtime-provider-key"
            try {
                $terminatingOutput = & $exe --approve --no-session --provider runtime-provider-smoke --model runtime-model-smoke --print "terminate tool smoke"
            } finally {
                $env:RUNTIME_PROVIDER_SMOKE_BASE_URL = $oldRuntimeBase
                $env:RUNTIME_PROVIDER_SMOKE_KEY = $oldRuntimeKey
            }
            if (($terminatingOutput -join "`n") -notmatch "terminate-ok mutated") {
                throw "tool_call hook did not mutate terminating extension tool arguments"
            }
            if (($terminatingOutput -join "`n") -notmatch "tool-result-hook-ok") {
                throw "tool_result hook did not modify terminating extension tool result"
            }
            if ((Get-Content $termRequestLog -Raw) -notmatch "terminating_tool_smoke") {
                throw "terminating extension tool was not advertised to provider"
            }
        } finally {
            Stop-Job $termServerJob -ErrorAction SilentlyContinue | Out-Null
            Remove-Job $termServerJob -Force -ErrorAction SilentlyContinue | Out-Null
        }

        Write-Host "== tool_call block smoke =="
        $blockPort = 20280 + (Get-Random -Minimum 0 -Maximum 1000)
        $blockRequestLog = Join-Path $SmokeRoot "tool-call-block-request.json"
        $blockReadyFile = Join-Path $SmokeRoot "tool-call-block-ready.txt"
        Remove-Item $blockRequestLog, $blockReadyFile -ErrorAction SilentlyContinue
        $blockServerJob = Start-Job -ArgumentList $blockPort, $blockRequestLog, $blockReadyFile -ScriptBlock {
            param($Port, $RequestLog, $ReadyFile)
            $listener = [System.Net.HttpListener]::new()
            $listener.Prefixes.Add("http://127.0.0.1:$Port/")
            try {
                $listener.Start()
                Set-Content -Path $ReadyFile -Value "ready" -Encoding UTF8
                for ($i = 0; $i -lt 2; $i++) {
                    $ctx = $listener.GetContext()
                    $reader = [System.IO.StreamReader]::new($ctx.Request.InputStream, $ctx.Request.ContentEncoding)
                    $body = $reader.ReadToEnd()
                    $reader.Close()
                    Add-Content -Path $RequestLog -Value "REQUEST-$i`n$body" -Encoding UTF8
                    if ($i -eq 0) {
                        $json = '{"id":"mock-block-1","choices":[{"message":{"role":"assistant","content":"","tool_calls":[{"id":"block-call-1","type":"function","function":{"name":"runtime_tool_smoke","arguments":"{\"value\":\"block-me\"}"}}]}}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}'
                    } else {
                        $json = '{"id":"mock-block-2","choices":[{"message":{"role":"assistant","content":"blocked-followup-ok"}}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}'
                    }
                    $bytes = [System.Text.Encoding]::UTF8.GetBytes($json)
                    $ctx.Response.StatusCode = 200
                    $ctx.Response.ContentType = "application/json"
                    $ctx.Response.ContentEncoding = [System.Text.Encoding]::UTF8
                    $ctx.Response.ContentLength64 = $bytes.Length
                    $ctx.Response.KeepAlive = $false
                    $ctx.Response.OutputStream.Write($bytes, 0, $bytes.Length)
                    $ctx.Response.OutputStream.Flush()
                    $ctx.Response.OutputStream.Close()
                }
            } finally {
                if ($listener.IsListening) {
                    $listener.Stop()
                }
                $listener.Close()
            }
        }
        try {
            for ($i = 0; $i -lt 80 -and -not (Test-Path $blockReadyFile); $i++) {
                Start-Sleep -Milliseconds 50
            }
            if (-not (Test-Path $blockReadyFile)) {
                throw "tool_call block mock server did not start"
            }
            $oldRuntimeBase = $env:RUNTIME_PROVIDER_SMOKE_BASE_URL
            $oldRuntimeKey = $env:RUNTIME_PROVIDER_SMOKE_KEY
            $env:RUNTIME_PROVIDER_SMOKE_BASE_URL = "http://127.0.0.1:$blockPort/v1"
            $env:RUNTIME_PROVIDER_SMOKE_KEY = "runtime-provider-key"
            try {
                $blockOutput = & $exe --approve --no-session --provider runtime-provider-smoke --model runtime-model-smoke --print "block tool smoke"
            } finally {
                $env:RUNTIME_PROVIDER_SMOKE_BASE_URL = $oldRuntimeBase
                $env:RUNTIME_PROVIDER_SMOKE_KEY = $oldRuntimeKey
            }
            if (($blockOutput -join "`n") -notmatch "blocked-followup-ok") {
                throw "tool_call block smoke did not complete follow-up turn"
            }
            $blockLog = Get-Content $blockRequestLog -Raw
            if ($blockLog -notmatch "blocked by hook") {
                throw "tool_call block reason did not reach provider as a tool result"
            }
            if ($blockLog -match "tool-ok block-me") {
                throw "blocked tool was executed"
            }
        } finally {
            Stop-Job $blockServerJob -ErrorAction SilentlyContinue | Out-Null
            Remove-Job $blockServerJob -Force -ErrorAction SilentlyContinue | Out-Null
        }
    } finally {
        Pop-Location
    }

    Write-Host "== local package settings smoke =="
    $packageProject = Join-Path $SmokeRoot "local-package-project"
    $localPackage = Join-Path $packageProject "local-pi-package"
    New-Item -ItemType Directory -Force -Path (Join-Path $packageProject ".pi") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $localPackage "extensions\pkg-extension") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $localPackage "prompts") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $localPackage "skills\pkg-skill") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $localPackage "themes") | Out-Null
    $packageThemeFile = Join-Path $localPackage "themes\pkg-theme.json"
    @{
        name = "local-pi-package"
        pi = @{
            extensions = @("./extensions")
            prompts = @("./prompts")
            skills = @("./skills")
            themes = @("./themes")
        }
    } | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $localPackage "package.json") -Encoding UTF8
    @{
        id = "pkg-extension"
        name = "Package Extension"
        commands = @{
            hello = @{
                description = "Package command"
                command = "Write-Output package-extension-ok"
            }
        }
    } | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $localPackage "extensions\pkg-extension\extension.json") -Encoding UTF8
    @(
        "---",
        "description: Package prompt smoke",
        "---",
        "Package prompt"
    ) -join "`n" | Set-Content -Path (Join-Path $localPackage "prompts\pkg-prompt.md") -Encoding UTF8
    @(
        "---",
        "name: pkg-skill",
        "description: Package skill smoke",
        "---",
        "Package skill"
    ) -join "`n" | Set-Content -Path (Join-Path $localPackage "skills\pkg-skill\SKILL.md") -Encoding UTF8
    @{
        name = "pkg-theme"
        colors = @{
            text = "#ffffff"
        }
    } | ConvertTo-Json -Depth 5 | Set-Content -Path $packageThemeFile -Encoding UTF8
    @{
        packages = @(
            @{
                source = "../local-pi-package"
                themes = @("./themes/pkg-theme.json")
            }
        )
    } | ConvertTo-Json | Set-Content -Path (Join-Path $packageProject ".pi\settings.json") -Encoding UTF8
    Push-Location $packageProject
    try {
        $packageSettings = & $exe --approve --no-session --print /settings
        if (($packageSettings -join "`n") -notmatch [regex]::Escape($localPackage)) {
            throw "settings.packages local package path was not shown"
        }
        if (($packageSettings -join "`n") -notmatch [regex]::Escape($packageThemeFile)) {
            throw "settings.packages theme filter did not add package theme path"
        }
        $packageThemes = & $exe --approve --no-session --print /themes
        if (($packageThemes -join "`n") -notmatch "pkg-theme") {
            throw "local package theme did not load"
        }
        $packageThemeDetail = & $exe --approve --no-session --print "/theme pkg-theme"
        if (($packageThemeDetail -join "`n") -notmatch [regex]::Escape($packageThemeFile)) {
            throw "local package theme detail did not show source path"
        }
        $packageExtensions = & $exe --approve --no-session --print /extensions
        if (($packageExtensions -join "`n") -notmatch "pkg-extension") {
            throw "local package extension did not load"
        }
        $packagePrompts = & $exe --approve --no-session --print /prompts
        if (($packagePrompts -join "`n") -notmatch "pkg-prompt") {
            throw "local package prompt did not load"
        }
        $packageSkills = & $exe --approve --no-session --print /skills
        if (($packageSkills -join "`n") -notmatch "pkg-skill") {
            throw "local package skill did not load"
        }
    } finally {
        Pop-Location
    }

    Write-Host "== resource settings smoke =="
    $resourceProject = Join-Path $SmokeRoot "resource-settings-project"
    $externalPrompts = Join-Path $SmokeRoot "external-prompts"
    $externalSkills = Join-Path $SmokeRoot "external-skills"
    $externalPromptFile = Join-Path $SmokeRoot "external-prompt-file.md"
    $externalSkillFile = Join-Path $SmokeRoot "external-skill-file.md"
    New-Item -ItemType Directory -Force -Path (Join-Path $resourceProject ".pi") | Out-Null
    New-Item -ItemType Directory -Force -Path $externalPrompts | Out-Null
    New-Item -ItemType Directory -Force -Path $externalSkills | Out-Null
    @(
        "---",
        "description: Settings prompt smoke",
        "---",
        "Prompt smoke $ARGUMENTS"
    ) -join "`n" | Set-Content -Path (Join-Path $externalPrompts "settings-prompt.md") -Encoding UTF8
    @(
        "---",
        "name: settings-skill",
        "description: Settings skill smoke",
        "---",
        "Skill smoke body"
    ) -join "`n" | Set-Content -Path (Join-Path $externalSkills "settings-skill.md") -Encoding UTF8
    @(
        "---",
        "description: Settings prompt file smoke",
        "---",
        "Prompt file smoke $ARGUMENTS"
    ) -join "`n" | Set-Content -Path $externalPromptFile -Encoding UTF8
    @(
        "---",
        "name: settings-file-skill",
        "description: Settings skill file smoke",
        "---",
        "Skill file smoke body"
    ) -join "`n" | Set-Content -Path $externalSkillFile -Encoding UTF8
    @{
        defaultThinkingLevel = "high"
        enableSkillCommands = $false
        prompts = @($externalPrompts, $externalPromptFile)
        skills = @($externalSkills, $externalSkillFile)
    } | ConvertTo-Json | Set-Content -Path (Join-Path $resourceProject ".pi\settings.json") -Encoding UTF8
    Push-Location $resourceProject
    try {
        $resourceSettings = & $exe --approve --no-session --print /settings
        if (($resourceSettings -join "`n") -notmatch "thinking\s+high") {
            throw "settings.defaultThinkingLevel did not set thinking level"
        }
        if (($resourceSettings -join "`n") -notmatch "enable_skill_commands\s+false") {
            throw "settings.enableSkillCommands did not set skill command toggle"
        }
        if (($resourceSettings -join "`n") -notmatch [regex]::Escape($externalPrompts)) {
            throw "settings.prompts path was not shown in /settings"
        }
        if (($resourceSettings -join "`n") -notmatch [regex]::Escape($externalSkills)) {
            throw "settings.skills path was not shown in /settings"
        }
        $promptList = & $exe --approve --no-session --print /prompts
        if (($promptList -join "`n") -notmatch "settings-prompt") {
            throw "settings.prompts did not load external prompt"
        }
        if (($promptList -join "`n") -notmatch "external-prompt-file") {
            throw "settings.prompts did not load external prompt file"
        }
        $skillList = & $exe --approve --no-session --print /skills
        if (($skillList -join "`n") -notmatch "settings-skill") {
            throw "settings.skills did not load external skill"
        }
        if (($skillList -join "`n") -notmatch "settings-file-skill") {
            throw "settings.skills did not load external skill file"
        }
    } finally {
        Pop-Location
    }

    Write-Host "== scoped relative resource path smoke =="
    $scopedProject = Join-Path $SmokeRoot "scoped-relative-resource-project"
    $projectRelativePrompts = Join-Path $scopedProject ".pi\relative-prompts"
    $projectRelativeSkills = Join-Path $scopedProject ".pi\relative-skills"
    New-Item -ItemType Directory -Force -Path $projectRelativePrompts | Out-Null
    New-Item -ItemType Directory -Force -Path $projectRelativeSkills | Out-Null
    @(
        "---",
        "description: Project scoped prompt smoke",
        "---",
        "Project scoped prompt"
    ) -join "`n" | Set-Content -Path (Join-Path $projectRelativePrompts "project-scoped.md") -Encoding UTF8
    @(
        "---",
        "name: project-scoped-skill",
        "description: Project scoped skill smoke",
        "---",
        "Project scoped skill"
    ) -join "`n" | Set-Content -Path (Join-Path $projectRelativeSkills "project-scoped-skill.md") -Encoding UTF8
    @{
        prompts = @("relative-prompts")
        skills = @("relative-skills")
    } | ConvertTo-Json | Set-Content -Path (Join-Path $scopedProject ".pi\settings.json") -Encoding UTF8
    Push-Location $scopedProject
    try {
        $scopedSettings = & $exe --approve --no-session --print /settings
        if (($scopedSettings -join "`n") -notmatch [regex]::Escape($projectRelativePrompts)) {
            throw "project settings.prompts relative path did not resolve from .pi"
        }
        if (($scopedSettings -join "`n") -notmatch [regex]::Escape($projectRelativeSkills)) {
            throw "project settings.skills relative path did not resolve from .pi"
        }
        $scopedPrompts = & $exe --approve --no-session --print /prompts
        if (($scopedPrompts -join "`n") -notmatch "project-scoped") {
            throw "project-scoped relative prompt did not load"
        }
        $scopedSkills = & $exe --approve --no-session --print /skills
        if (($scopedSkills -join "`n") -notmatch "project-scoped-skill") {
            throw "project-scoped relative skill did not load"
        }
    } finally {
        Pop-Location
    }

    Write-Host "== resource pattern settings smoke =="
    $patternProject = Join-Path $SmokeRoot "resource-pattern-settings-project"
    $patternPrompts = Join-Path $patternProject ".pi\pattern-prompts"
    New-Item -ItemType Directory -Force -Path $patternPrompts | Out-Null
    @(
        "---",
        "description: Pattern keep prompt",
        "---",
        "Pattern keep"
    ) -join "`n" | Set-Content -Path (Join-Path $patternPrompts "keep.md") -Encoding UTF8
    @(
        "---",
        "description: Pattern skip prompt",
        "---",
        "Pattern skip"
    ) -join "`n" | Set-Content -Path (Join-Path $patternPrompts "skip.md") -Encoding UTF8
    @(
        "---",
        "description: Pattern force prompt",
        "---",
        "Pattern force"
    ) -join "`n" | Set-Content -Path (Join-Path $patternPrompts "force.md") -Encoding UTF8
    @{
        prompts = @("pattern-prompts", "*.md", "!skip.md", "-pattern-prompts/force.md")
    } | ConvertTo-Json | Set-Content -Path (Join-Path $patternProject ".pi\settings.json") -Encoding UTF8
    Push-Location $patternProject
    try {
        $patternList = & $exe --approve --no-session --print /prompts
        if (($patternList -join "`n") -notmatch "keep") {
            throw "settings.prompts pattern did not include matching prompt"
        }
        if (($patternList -join "`n") -match "skip") {
            throw "settings.prompts ! pattern did not exclude prompt"
        }
        if (($patternList -join "`n") -match "force") {
            throw "settings.prompts - pattern did not force-exclude prompt"
        }
    } finally {
        Pop-Location
    }

    Write-Host "== agents skills smoke =="
    $agentsProject = Join-Path $SmokeRoot "agents-skills-project"
    $agentsSkillDir = Join-Path $agentsProject ".agents\skills\reviewer"
    New-Item -ItemType Directory -Force -Path $agentsSkillDir | Out-Null
    @(
        "---",
        "name: agents-reviewer",
        "description: Agents skill smoke",
        "---",
        "Agents skill body"
    ) -join "`n" | Set-Content -Path (Join-Path $agentsSkillDir "SKILL.md") -Encoding UTF8
    Push-Location $agentsProject
    try {
        $agentsSkills = & $exe --approve --no-session --print /skills
        if (($agentsSkills -join "`n") -notmatch "agents-reviewer") {
            throw ".agents/skills did not auto-load project skill"
        }
    } finally {
        Pop-Location
    }

    Write-Host "== global relative resource path smoke =="
    $globalRelativeProject = Join-Path $SmokeRoot "global-relative-resource-project"
    $agentDir = $env:PI_CODING_AGENT_DIR
    $agentSettingsPath = Join-Path $agentDir "settings.json"
    $globalRelativePrompts = Join-Path $agentDir "global-relative-prompts"
    New-Item -ItemType Directory -Force -Path $globalRelativeProject | Out-Null
    New-Item -ItemType Directory -Force -Path $globalRelativePrompts | Out-Null
    @(
        "---",
        "description: Global scoped prompt smoke",
        "---",
        "Global scoped prompt"
    ) -join "`n" | Set-Content -Path (Join-Path $globalRelativePrompts "global-scoped.md") -Encoding UTF8
    @{
        prompts = @("global-relative-prompts")
    } | ConvertTo-Json | Set-Content -Path $agentSettingsPath -Encoding UTF8
    Push-Location $globalRelativeProject
    try {
        $globalSettings = & $exe --no-session --print /settings
        if (($globalSettings -join "`n") -notmatch [regex]::Escape($globalRelativePrompts)) {
            throw "global settings.prompts relative path did not resolve from agent dir"
        }
        $globalPrompts = & $exe --no-session --print /prompts
        if (($globalPrompts -join "`n") -notmatch "global-scoped") {
            throw "global-scoped relative prompt did not load"
        }
    } finally {
        Pop-Location
        Remove-Item -LiteralPath $agentSettingsPath -Force -ErrorAction SilentlyContinue
    }

    Write-Host "== tilde path settings smoke =="
    $tildeProject = Join-Path $SmokeRoot "tilde-settings-project"
    $homeSmokeRoot = Join-Path $HOME ".bbarit-tilde-smoke"
    $homePromptDir = Join-Path $homeSmokeRoot "prompts"
    $homeSessionDir = Join-Path $homeSmokeRoot "sessions"
    Remove-Item -LiteralPath $homeSmokeRoot -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force -Path (Join-Path $tildeProject ".pi") | Out-Null
    New-Item -ItemType Directory -Force -Path $homePromptDir | Out-Null
    @(
        "---",
        "description: Tilde prompt smoke",
        "---",
        "Tilde prompt $ARGUMENTS"
    ) -join "`n" | Set-Content -Path (Join-Path $homePromptDir "tilde-prompt.md") -Encoding UTF8
    @{
        sessionDir = "~/.bbarit-tilde-smoke/sessions"
        prompts = @("~/.bbarit-tilde-smoke/prompts")
    } | ConvertTo-Json | Set-Content -Path (Join-Path $tildeProject ".pi\settings.json") -Encoding UTF8
    Push-Location $tildeProject
    try {
        $tildeSettings = & $exe --approve --no-session --print /settings
        if (($tildeSettings -join "`n") -notmatch [regex]::Escape($homeSessionDir)) {
            throw "settings.sessionDir did not expand tilde"
        }
        if (($tildeSettings -join "`n") -notmatch [regex]::Escape($homePromptDir)) {
            throw "settings.prompts did not expand tilde"
        }
        $tildePrompts = & $exe --approve --no-session --print /prompts
        if (($tildePrompts -join "`n") -notmatch "tilde-prompt") {
            throw "tilde-expanded prompt path did not load prompt"
        }
    } finally {
        Pop-Location
        Remove-Item -LiteralPath $homeSmokeRoot -Recurse -Force -ErrorAction SilentlyContinue
    }

    Write-Host "== legacy skills settings smoke =="
    $legacySkillsProject = Join-Path $SmokeRoot "legacy-skills-settings-project"
    New-Item -ItemType Directory -Force -Path (Join-Path $legacySkillsProject ".pi") | Out-Null
    @{
        skills = @{
            enableSkillCommands = $false
            customDirectories = @($externalSkills)
        }
    } | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $legacySkillsProject ".pi\settings.json") -Encoding UTF8
    Push-Location $legacySkillsProject
    try {
        $legacySettings = & $exe --approve --no-session --print /settings
        if (($legacySettings -join "`n") -notmatch "enable_skill_commands\s+false") {
            throw "legacy skills.enableSkillCommands was not migrated"
        }
        if (($legacySettings -join "`n") -notmatch [regex]::Escape($externalSkills)) {
            throw "legacy skills.customDirectories path was not shown in /settings"
        }
        $legacySkillList = & $exe --approve --no-session --print /skills
        if (($legacySkillList -join "`n") -notmatch "settings-skill") {
            throw "legacy skills.customDirectories did not load external skill"
        }
    } finally {
        Pop-Location
    }

    Write-Host "== project trust loading smoke =="
    $trustLoadingProject = Join-Path $SmokeRoot "trust-loading-project"
    New-Item -ItemType Directory -Force -Path (Join-Path $trustLoadingProject ".pi\prompts") | Out-Null
    @(
        "---",
        "description: Trust gated prompt",
        "---",
        "Trust gated prompt"
    ) -join "`n" | Set-Content -Path (Join-Path $trustLoadingProject ".pi\prompts\trust-gated.md") -Encoding UTF8
    Push-Location $trustLoadingProject
    try {
        $untrustedSettings = & $exe --no-session --print /settings
        if (($untrustedSettings -join "`n") -notmatch "project_trusted\s+false") {
            throw "untrusted project did not report project_trusted false"
        }
        $untrustedPrompts = & $exe --no-session --print /prompts
        if (($untrustedPrompts -join "`n") -match "trust-gated") {
            throw "untrusted project loaded project prompt"
        }
        $approvedPrompts = & $exe --approve --no-session --print /prompts
        if (($approvedPrompts -join "`n") -notmatch "trust-gated") {
            throw "--approve did not load project prompt"
        }
        $noApprovePrompts = & $exe --no-approve --no-session --print /prompts
        if (($noApprovePrompts -join "`n") -match "trust-gated") {
            throw "--no-approve loaded project prompt"
        }
    } finally {
        Pop-Location
    }

    Write-Host "== default project trust smoke =="
    $defaultTrustProject = Join-Path $SmokeRoot "default-project-trust-project"
    $defaultTrustAgentSettings = Join-Path $env:PI_CODING_AGENT_DIR "settings.json"
    New-Item -ItemType Directory -Force -Path (Join-Path $defaultTrustProject ".pi\prompts") | Out-Null
    @(
        "---",
        "description: Default trust prompt",
        "---",
        "Default trust prompt"
    ) -join "`n" | Set-Content -Path (Join-Path $defaultTrustProject ".pi\prompts\default-trust.md") -Encoding UTF8
    @{
        defaultProjectTrust = "always"
    } | ConvertTo-Json | Set-Content -Path $defaultTrustAgentSettings -Encoding UTF8
    Push-Location $defaultTrustProject
    try {
        $defaultTrustPrompts = & $exe --no-session --print /prompts
        if (($defaultTrustPrompts -join "`n") -notmatch "default-trust") {
            throw "defaultProjectTrust=always did not load project prompt"
        }
        $noApproveDefaultTrustPrompts = & $exe -na --no-session --print /prompts
        if (($noApproveDefaultTrustPrompts -join "`n") -match "default-trust") {
            throw "-na did not override defaultProjectTrust=always"
        }
    } finally {
        Pop-Location
        Remove-Item -LiteralPath $defaultTrustAgentSettings -Force -ErrorAction SilentlyContinue
    }

    Write-Host "== bash settings smoke =="
    $bashProject = Join-Path $SmokeRoot "bash-settings-project"
    New-Item -ItemType Directory -Force -Path (Join-Path $bashProject ".pi") | Out-Null
    $smokeShellPath = (Get-Command powershell).Source
    @{
        shellPath = $smokeShellPath
        shellCommandPrefix = "Write-Output prefix-ok"
    } | ConvertTo-Json | Set-Content -Path (Join-Path $bashProject ".pi\settings.json") -Encoding UTF8
    Push-Location $bashProject
    try {
        & $exe --no-session --print "/trust yes"
        if ($LASTEXITCODE -ne 0) {
            throw "trust yes failed for bash settings smoke"
        }
        $bashOutput = & $exe --no-session --print "/bash Write-Output command-ok"
        if ($LASTEXITCODE -ne 0) {
            throw "bash command with shellCommandPrefix failed"
        }
        if (($bashOutput -join "`n") -notmatch "prefix-ok") {
            throw "shellCommandPrefix did not run before bash command"
        }
        if (($bashOutput -join "`n") -notmatch "command-ok") {
            throw "bash command did not run after shellCommandPrefix"
        }
        $cwdOutput = & $exe --no-session --print "/bash Get-Location"
        if ($LASTEXITCODE -ne 0) {
            throw "bash cwd command failed"
        }
        $expectedCwd = [regex]::Escape((Resolve-Path $bashProject).Path)
        if (($cwdOutput -join "`n") -notmatch $expectedCwd) {
            throw "bash did not run in project cwd"
        }
    } finally {
        Pop-Location
    }

    Write-Host "== trust smoke =="
    $trustDir = Join-Path $SmokeRoot "trust-project"
    New-Item -ItemType Directory -Force -Path (Join-Path $trustDir ".pi\prompts") | Out-Null
    Push-Location $trustDir
    try {
        $oldErrorActionPreference = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        $blocked = & $exe --no-session --print "/bash Write-Output should-not-run" 2>&1
        $blockedExitCode = $LASTEXITCODE
        $ErrorActionPreference = $oldErrorActionPreference
        if ($blockedExitCode -eq 0) {
            throw "untrusted project allowed bash"
        }
        if (($blocked -join "`n") -notmatch "not trusted") {
            throw "untrusted project did not report trust error"
        }
        & $exe --no-session --print "/trust yes"
        if ($LASTEXITCODE -ne 0) {
            throw "trust yes failed"
        }
        $allowed = & $exe --no-session --print "/bash Write-Output trusted-ok" 2>&1
        if ($LASTEXITCODE -ne 0) {
            throw "trusted project blocked bash"
        }
        if (($allowed -join "`n") -notmatch "trusted-ok") {
            throw "trusted project bash did not run"
        }
    } finally {
        Pop-Location
    }

    Write-Host "== tui (line-REPL fallback) smoke =="
    $tuiOutput = Join-Path $SmokeRoot "tui-screen.txt"
    @('/session','/help','/exit') | & $exe --tui --session-dir (Join-Path $SmokeRoot "tui-sessions") *> $tuiOutput
    Assert-TerminalText $tuiOutput "/model" "TUI line-REPL did not run /help"
    Assert-TerminalText $tuiOutput "Cost" "TUI line-REPL did not run /session"
    Write-Host "TUI output: $tuiOutput"

    Write-Host "== tui model + sessions smoke =="
    $modelOutput = Join-Path $SmokeRoot "tui-model.txt"
    @('/models ollama','/sessions','/exit') | & $exe --tui --session-dir (Join-Path $SmokeRoot "tui-model-sessions") *> $modelOutput
    Assert-TerminalText $modelOutput "ollama" "TUI line-REPL /models did not list ollama"
    Write-Host "TUI model output: $modelOutput"

    Write-Host "== ok =="
} finally {
    Pop-Location
}







