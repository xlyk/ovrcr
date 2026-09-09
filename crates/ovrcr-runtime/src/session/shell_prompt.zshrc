# Normal configuration sees its real ZDOTDIR, also inherited by nested shells.
if (( _ovrcr_original_zdotdir_set )); then
  ZDOTDIR=$_ovrcr_original_zdotdir
else
  unset ZDOTDIR
fi
unset _ovrcr_original_zdotdir_set _ovrcr_original_zdotdir
[[ -r ${ZDOTDIR-$HOME}/.zshrc ]] && source "${ZDOTDIR-$HOME}/.zshrc"

# Stop Powerlevel10k's redraw hooks before replacing its prompt.
(( $+functions[p10k-instant-prompt-finalize] )) && p10k-instant-prompt-finalize
(( $+functions[prompt_powerlevel9k_teardown] )) && prompt_powerlevel9k_teardown
setopt prompt_percent
unsetopt prompt_subst
PROMPT='%F{6}%1~%f %(?.%F{7}.%F{1})›%f '
RPROMPT=''
