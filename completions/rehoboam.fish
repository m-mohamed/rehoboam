# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_rehoboam_global_optspecs
	string join \n s/socket= l/log-level= d/debug t/tick-rate= F/frame-rate= install enable-sprites sprites-token= sprite-ws-port= h/help V/version
end

function __fish_rehoboam_needs_command
	# Figure out if the current invocation already has a command.
	set -l cmd (commandline -opc)
	set -e cmd[1]
	argparse -s (__fish_rehoboam_global_optspecs) -- $cmd 2>/dev/null
	or return
	if set -q argv[1]
		# Also print the command, so this can be used to figure out what it is.
		echo $argv[1]
		return 1
	end
	return 0
end

function __fish_rehoboam_using_subcommand
	set -l cmd (__fish_rehoboam_needs_command)
	test -z "$cmd"
	and return 1
	contains -- $cmd[1] $argv
end

complete -c rehoboam -n "__fish_rehoboam_needs_command" -s s -l socket -d 'Socket path for receiving hook events (default: $XDG_RUNTIME_DIR/rehoboam.sock or /tmp/rehoboam.sock)' -r -F
complete -c rehoboam -n "__fish_rehoboam_needs_command" -s l -l log-level -d 'Log level (trace, debug, info, warn, error)' -r
complete -c rehoboam -n "__fish_rehoboam_needs_command" -s t -l tick-rate -d 'Tick rate in ticks per second (default: 1.0)' -r
complete -c rehoboam -n "__fish_rehoboam_needs_command" -s F -l frame-rate -d 'Frame rate in frames per second (default: 30.0)' -r
complete -c rehoboam -n "__fish_rehoboam_needs_command" -l sprites-token -d 'Sprites API token (required if --enable-sprites)' -r
complete -c rehoboam -n "__fish_rehoboam_needs_command" -l sprite-ws-port -d 'WebSocket port for receiving hook events from remote sprites' -r
complete -c rehoboam -n "__fish_rehoboam_needs_command" -s d -l debug -d 'Run in debug mode (shows event log)'
complete -c rehoboam -n "__fish_rehoboam_needs_command" -l install -d 'Install binary to ~/.local/bin/'
complete -c rehoboam -n "__fish_rehoboam_needs_command" -l enable-sprites -d 'Enable remote sprite support (run Claude Code in sandboxed VMs)'
complete -c rehoboam -n "__fish_rehoboam_needs_command" -s h -l help -d 'Print help'
complete -c rehoboam -n "__fish_rehoboam_needs_command" -s V -l version -d 'Print version'
complete -c rehoboam -n "__fish_rehoboam_needs_command" -f -a "hook" -d 'Process Claude Code hook JSON from stdin (v1.0+)'
complete -c rehoboam -n "__fish_rehoboam_needs_command" -f -a "init" -d 'Install Claude Code hooks to a project'
complete -c rehoboam -n "__fish_rehoboam_needs_command" -f -a "completions" -d 'Generate shell completions'
complete -c rehoboam -n "__fish_rehoboam_needs_command" -f -a "sprites" -d 'Manage remote sprites (cloud VMs)'
complete -c rehoboam -n "__fish_rehoboam_needs_command" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand hook" -s s -l socket -d 'Socket path for receiving hook events (default: $XDG_RUNTIME_DIR/rehoboam.sock or /tmp/rehoboam.sock)' -r -F
complete -c rehoboam -n "__fish_rehoboam_using_subcommand hook" -s l -l log-level -d 'Log level (trace, debug, info, warn, error)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand hook" -s t -l tick-rate -d 'Tick rate in ticks per second (default: 1.0)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand hook" -s F -l frame-rate -d 'Frame rate in frames per second (default: 30.0)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand hook" -l sprites-token -d 'Sprites API token (required if --enable-sprites)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand hook" -l sprite-ws-port -d 'WebSocket port for receiving hook events from remote sprites' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand hook" -s N -l notify -d 'Send desktop notification (for attention and stop events)'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand hook" -s d -l debug -d 'Run in debug mode (shows event log)'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand hook" -l enable-sprites -d 'Enable remote sprite support (run Claude Code in sandboxed VMs)'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand hook" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand hook" -s V -l version -d 'Print version'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand init" -s s -l socket -d 'Socket path for receiving hook events (default: $XDG_RUNTIME_DIR/rehoboam.sock or /tmp/rehoboam.sock)' -r -F
complete -c rehoboam -n "__fish_rehoboam_using_subcommand init" -s l -l log-level -d 'Log level (trace, debug, info, warn, error)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand init" -s t -l tick-rate -d 'Tick rate in ticks per second (default: 1.0)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand init" -s F -l frame-rate -d 'Frame rate in frames per second (default: 30.0)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand init" -l sprites-token -d 'Sprites API token (required if --enable-sprites)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand init" -l sprite-ws-port -d 'WebSocket port for receiving hook events from remote sprites' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand init" -l all -d 'Discover and select from git repos interactively'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand init" -l list -d 'List discovered git repositories'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand init" -l force -d 'Force overwrite existing hooks (default: merge)'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand init" -s d -l debug -d 'Run in debug mode (shows event log)'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand init" -l enable-sprites -d 'Enable remote sprite support (run Claude Code in sandboxed VMs)'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand init" -s h -l help -d 'Print help'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand init" -s V -l version -d 'Print version'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand completions" -s s -l socket -d 'Socket path for receiving hook events (default: $XDG_RUNTIME_DIR/rehoboam.sock or /tmp/rehoboam.sock)' -r -F
complete -c rehoboam -n "__fish_rehoboam_using_subcommand completions" -s l -l log-level -d 'Log level (trace, debug, info, warn, error)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand completions" -s t -l tick-rate -d 'Tick rate in ticks per second (default: 1.0)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand completions" -s F -l frame-rate -d 'Frame rate in frames per second (default: 30.0)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand completions" -l sprites-token -d 'Sprites API token (required if --enable-sprites)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand completions" -l sprite-ws-port -d 'WebSocket port for receiving hook events from remote sprites' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand completions" -s d -l debug -d 'Run in debug mode (shows event log)'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand completions" -l enable-sprites -d 'Enable remote sprite support (run Claude Code in sandboxed VMs)'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand completions" -s h -l help -d 'Print help'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand completions" -s V -l version -d 'Print version'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and not __fish_seen_subcommand_from list info destroy destroy-all help" -s s -l socket -d 'Socket path for receiving hook events (default: $XDG_RUNTIME_DIR/rehoboam.sock or /tmp/rehoboam.sock)' -r -F
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and not __fish_seen_subcommand_from list info destroy destroy-all help" -s l -l log-level -d 'Log level (trace, debug, info, warn, error)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and not __fish_seen_subcommand_from list info destroy destroy-all help" -s t -l tick-rate -d 'Tick rate in ticks per second (default: 1.0)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and not __fish_seen_subcommand_from list info destroy destroy-all help" -s F -l frame-rate -d 'Frame rate in frames per second (default: 30.0)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and not __fish_seen_subcommand_from list info destroy destroy-all help" -l sprites-token -d 'Sprites API token (required if --enable-sprites)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and not __fish_seen_subcommand_from list info destroy destroy-all help" -l sprite-ws-port -d 'WebSocket port for receiving hook events from remote sprites' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and not __fish_seen_subcommand_from list info destroy destroy-all help" -s d -l debug -d 'Run in debug mode (shows event log)'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and not __fish_seen_subcommand_from list info destroy destroy-all help" -l enable-sprites -d 'Enable remote sprite support (run Claude Code in sandboxed VMs)'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and not __fish_seen_subcommand_from list info destroy destroy-all help" -s h -l help -d 'Print help'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and not __fish_seen_subcommand_from list info destroy destroy-all help" -s V -l version -d 'Print version'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and not __fish_seen_subcommand_from list info destroy destroy-all help" -f -a "list" -d 'List all sprites with status'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and not __fish_seen_subcommand_from list info destroy destroy-all help" -f -a "info" -d 'Show detailed info for a sprite'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and not __fish_seen_subcommand_from list info destroy destroy-all help" -f -a "destroy" -d 'Destroy a sprite (frees all resources)'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and not __fish_seen_subcommand_from list info destroy destroy-all help" -f -a "destroy-all" -d 'Destroy all sprites'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and not __fish_seen_subcommand_from list info destroy destroy-all help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from list" -s s -l socket -d 'Socket path for receiving hook events (default: $XDG_RUNTIME_DIR/rehoboam.sock or /tmp/rehoboam.sock)' -r -F
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from list" -s l -l log-level -d 'Log level (trace, debug, info, warn, error)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from list" -s t -l tick-rate -d 'Tick rate in ticks per second (default: 1.0)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from list" -s F -l frame-rate -d 'Frame rate in frames per second (default: 30.0)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from list" -l sprites-token -d 'Sprites API token (required if --enable-sprites)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from list" -l sprite-ws-port -d 'WebSocket port for receiving hook events from remote sprites' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from list" -s d -l debug -d 'Run in debug mode (shows event log)'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from list" -l enable-sprites -d 'Enable remote sprite support (run Claude Code in sandboxed VMs)'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from list" -s h -l help -d 'Print help'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from list" -s V -l version -d 'Print version'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from info" -s s -l socket -d 'Socket path for receiving hook events (default: $XDG_RUNTIME_DIR/rehoboam.sock or /tmp/rehoboam.sock)' -r -F
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from info" -s l -l log-level -d 'Log level (trace, debug, info, warn, error)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from info" -s t -l tick-rate -d 'Tick rate in ticks per second (default: 1.0)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from info" -s F -l frame-rate -d 'Frame rate in frames per second (default: 30.0)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from info" -l sprites-token -d 'Sprites API token (required if --enable-sprites)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from info" -l sprite-ws-port -d 'WebSocket port for receiving hook events from remote sprites' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from info" -s d -l debug -d 'Run in debug mode (shows event log)'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from info" -l enable-sprites -d 'Enable remote sprite support (run Claude Code in sandboxed VMs)'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from info" -s h -l help -d 'Print help'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from info" -s V -l version -d 'Print version'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from destroy" -s s -l socket -d 'Socket path for receiving hook events (default: $XDG_RUNTIME_DIR/rehoboam.sock or /tmp/rehoboam.sock)' -r -F
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from destroy" -s l -l log-level -d 'Log level (trace, debug, info, warn, error)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from destroy" -s t -l tick-rate -d 'Tick rate in ticks per second (default: 1.0)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from destroy" -s F -l frame-rate -d 'Frame rate in frames per second (default: 30.0)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from destroy" -l sprites-token -d 'Sprites API token (required if --enable-sprites)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from destroy" -l sprite-ws-port -d 'WebSocket port for receiving hook events from remote sprites' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from destroy" -s d -l debug -d 'Run in debug mode (shows event log)'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from destroy" -l enable-sprites -d 'Enable remote sprite support (run Claude Code in sandboxed VMs)'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from destroy" -s h -l help -d 'Print help'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from destroy" -s V -l version -d 'Print version'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from destroy-all" -s s -l socket -d 'Socket path for receiving hook events (default: $XDG_RUNTIME_DIR/rehoboam.sock or /tmp/rehoboam.sock)' -r -F
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from destroy-all" -s l -l log-level -d 'Log level (trace, debug, info, warn, error)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from destroy-all" -s t -l tick-rate -d 'Tick rate in ticks per second (default: 1.0)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from destroy-all" -s F -l frame-rate -d 'Frame rate in frames per second (default: 30.0)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from destroy-all" -l sprites-token -d 'Sprites API token (required if --enable-sprites)' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from destroy-all" -l sprite-ws-port -d 'WebSocket port for receiving hook events from remote sprites' -r
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from destroy-all" -s y -l yes -d 'Skip confirmation'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from destroy-all" -s d -l debug -d 'Run in debug mode (shows event log)'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from destroy-all" -l enable-sprites -d 'Enable remote sprite support (run Claude Code in sandboxed VMs)'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from destroy-all" -s h -l help -d 'Print help'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from destroy-all" -s V -l version -d 'Print version'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from help" -f -a "list" -d 'List all sprites with status'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from help" -f -a "info" -d 'Show detailed info for a sprite'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from help" -f -a "destroy" -d 'Destroy a sprite (frees all resources)'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from help" -f -a "destroy-all" -d 'Destroy all sprites'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand sprites; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand help; and not __fish_seen_subcommand_from hook init completions sprites help" -f -a "hook" -d 'Process Claude Code hook JSON from stdin (v1.0+)'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand help; and not __fish_seen_subcommand_from hook init completions sprites help" -f -a "init" -d 'Install Claude Code hooks to a project'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand help; and not __fish_seen_subcommand_from hook init completions sprites help" -f -a "completions" -d 'Generate shell completions'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand help; and not __fish_seen_subcommand_from hook init completions sprites help" -f -a "sprites" -d 'Manage remote sprites (cloud VMs)'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand help; and not __fish_seen_subcommand_from hook init completions sprites help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand help; and __fish_seen_subcommand_from sprites" -f -a "list" -d 'List all sprites with status'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand help; and __fish_seen_subcommand_from sprites" -f -a "info" -d 'Show detailed info for a sprite'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand help; and __fish_seen_subcommand_from sprites" -f -a "destroy" -d 'Destroy a sprite (frees all resources)'
complete -c rehoboam -n "__fish_rehoboam_using_subcommand help; and __fish_seen_subcommand_from sprites" -f -a "destroy-all" -d 'Destroy all sprites'
