# Restore the user's startup location while reading their environment.
_ovrcr_startup_dir=$ZDOTDIR
if (( ${+OVRCR_ORIGINAL_ZDOTDIR} )); then
  ZDOTDIR=$OVRCR_ORIGINAL_ZDOTDIR
else
  unset ZDOTDIR
fi
unset OVRCR_ORIGINAL_ZDOTDIR
[[ -r ${ZDOTDIR-$HOME}/.zshenv ]] && source "${ZDOTDIR-$HOME}/.zshenv"
_ovrcr_original_zdotdir_set=${+ZDOTDIR}
_ovrcr_original_zdotdir=${ZDOTDIR-}
ZDOTDIR=$_ovrcr_startup_dir
unset _ovrcr_startup_dir
