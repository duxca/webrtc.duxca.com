.PHONY:fmt
fmt:
	cargo fmt
	find . -name Cargo.toml -execdir cargo tomlfmt \;
