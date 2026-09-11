# Accessibility text normalization

The initial staged whitespace check failed on terminal padding in the accessibility captures. Only trailing whitespace on each line and extra final blank lines were removed. Visible text, ordering, leading indentation, and screenshots were not changed. The seed AX file is the CUA state diff; resumed-output and missing-UUID AX files are full fresh trees.
