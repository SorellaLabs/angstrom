cloc $(find src \
    -name "*.sol" \
    -not -path "src/libraries/Create2Lib.sol" \
) --by-file
