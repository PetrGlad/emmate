#!/bin/bash

COMPLETIONS=~/.local/share/bash-completion/completions/
mkdir --parents "$COMPLETIONS"

NAME=emmate
"$NAME" --shell-completion-script=bash >"$COMPLETIONS/$NAME"
