#!/bin/sh
# Deterministic native provider for delegation acceptance. The runtime, dispatcher,
# reporter, tools, PTYs and files are all production; this replaces only model work.
case " $* " in
  *" --list-models "*) printf 'provider model context\ntest fixture-model 100K\ntest alternate-model 100K\n'; exit 0 ;;
esac
exec node "${OVRCR_TEST_PI_HOST:?set to tests/fixtures/pi/pi_host.mjs}" "$@"
