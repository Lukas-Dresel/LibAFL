.PHONY: all


CFLAGS_aflpp_driver=-I../../../repos/AFLPlusplus/include ../../../repos/AFLPlusplus/utils/aflpp_driver/aflpp_driver.c
CFLAGS=-Wno-sign-compare -funroll-loops -unroll-count=16 -O1 -g
SANITIZER_CFLAGS=-fsanitize=undefined -fsanitize=array-bounds,bool,builtin,enum,float-divide-by-zero,function,integer-divide-by-zero,null,object-size,return,returns-nonnull-attribute,shift,signed-integer-overflow,unreachable,vla-bound,vptr -fno-sanitize-recover=all

ALL_TARGETS=target_symcc target_symcts harness_symcc_coverage harness_symcc_sanitized harness_symcts_afl++ harness_symcts_afl++.cmplog harness_afl++ harness_afl++.cmplog

all: $(ALL_TARGETS)
clean:
	rm -f $(ALL_TARGETS)

target_symcc: ./harness_stb.c ./stb_image.h Makefile
	../../../repos/symcc/build_qsym/symcc $(CFLAGS) $(CFLAGS_aflpp_driver) -fsanitize=undefined -fsanitize-trap=all -o $@ $< -L../../../repos/symcc_libc_preload/ $(LDFLAGS) -lm -l:libc_symcc_preload.a

target_symcts: ./harness_stb.c ./stb_image.h Makefile
	SYMCC_RUNTIME_DIR=../runtime/target/release/ ../../../repos/symcc/build/symcc $(CFLAGS) $(CFLAGS_aflpp_driver) -fsanitize=undefined -fsanitize-trap=all -o $@ $< -L../../../repos/symcc_libc_preload/ $(LDFLAGS) -lm -l:libc_symcc_preload.a

harness_symcc_coverage: harness_stb.c Makefile stb_image.h
	clang $(CFLAGS) $(CFLAGS_aflpp_driver) -o $@ $< $(LDFLAGS) -lm

harness_symcc_sanitized: harness_stb.c Makefile stb_image.h
	clang $(SANITIZER_CFLAGS) $(CFLAGS) $(CFLAGS_aflpp_driver) -o $@ $< $(LDFLAGS) -lm

harness_symcts_afl++: harness_stb.c Makefile stb_image.h
	../../../repos/AFLPlusplus/afl-clang-fast $(SANITIZER_CFLAGS) $(CFLAGS) ../../../repos/AFLPlusplus/libAFLDriver.a -o $@ $< $(LDFLAGS) -lm

harness_symcts_afl++.cmplog: harness_stb.c Makefile stb_image.h
	AFL_LLVM_CMPLOG=1 ../../../repos/AFLPlusplus/afl-clang-fast $(CFLAGS) ../../../repos/AFLPlusplus/libAFLDriver.a -o $@ $< $(LDFLAGS) -lm


harness_afl++: harness_stb.c Makefile stb_image.h
	../../../repos/AFLPlusplus_upstream/afl-clang-fast $(SANITIZER_CFLAGS) $(CFLAGS) ../../../repos/AFLPlusplus_upstream/libAFLDriver.a -o $@ $< $(LDFLAGS) -lm

harness_afl++.cmplog: harness_stb.c Makefile stb_image.h
	AFL_LLVM_CMPLOG=1 ../../../repos/AFLPlusplus_upstream/afl-clang-fast $(CFLAGS) ../../../repos/AFLPlusplus_upstream/libAFLDriver.a -o $@ $< $(LDFLAGS) -lm
