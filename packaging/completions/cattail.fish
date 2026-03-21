complete -c cattail -s n -l lines -d 'Number of backlog lines to print per startup-resolved file' -r
complete -c cattail -l interval-ms -d 'Polling interval in milliseconds for recovery scans and file reopen checks' -r
complete -c cattail -l prefix -d 'How to label each output line' -r -f -a "basename\t''
relative\t''
full\t''"
complete -c cattail -l color -d 'Colorize line prefixes when writing to an interactive terminal' -r -f -a "auto\t''
always\t''
never\t''"
complete -c cattail -l since-now -d 'Skip the backlog and only emit lines appended after startup'
complete -c cattail -s h -l help -d 'Print help (see more with \'--help\')'
complete -c cattail -s V -l version -d 'Print version'
