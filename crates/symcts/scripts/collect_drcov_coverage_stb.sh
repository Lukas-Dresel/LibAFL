#!/bin/bash

export DRHOME=/home/honululu/lukas/tools/DynamoRIO-Linux-8.0.0-1/bin64

function collect_coverage_single() {
    # set -x
    INPUT=$1
    OUTDIR=$2
    mkdir -p "$OUTDIR"
    timeout 10 $DRHOME/drrun -t drcov -logdir "$OUTDIR" -- ./harness_symcc_coverage "$INPUT" 1>/dev/null 2>/dev/null
    # set +x
}

function collect_coverage() {
    INDIR=$1
    OUTDIR=$2
    mkdir -p "$2"
    for f in `find $1 \( ! -regex '.*/\.[^/]*' \) | sort`; do
        echo "Processing $f ..."
        collect_coverage_single "$f" "$OUTDIR"
    done
}

collect_coverage ./corpus_oss_fuzz/stb/stbi_read_fuzzer /media/honululu/Data/cov_drcov_stb/oss_fuzz_saturated

# collect_coverage ./local_experiment_new_coverage_1w_cmplog/minimized_union_corpus/afl/corpus /media/honululu/Data/cov_drcov_stb/cmplog_afl
# collect_coverage ./local_experiment_new_coverage_1w_cmplog/minimized_union_corpus/afl/crashes /media/honululu/Data/cov_drcov_stb/cmplog_afl
# collect_coverage ./local_experiment_new_coverage_1w_cmplog/minimized_union_corpus/symcc_afl/corpus /media/honululu/Data/cov_drcov_stb/cmplog_symcc_afl
# collect_coverage ./local_experiment_new_coverage_1w_cmplog/minimized_union_corpus/symcc_afl/crashes /media/honululu/Data/cov_drcov_stb/cmplog_symcc_afl
# collect_coverage ./local_experiment_new_coverage_1w_cmplog/minimized_union_corpus/symcts/corpus /media/honululu/Data/cov_drcov_stb/cmplog_symcts
# collect_coverage ./local_experiment_new_coverage_1w_cmplog/minimized_union_corpus/symcts/crashes /media/honululu/Data/cov_drcov_stb/cmplog_symcts
# collect_coverage ./local_experiment_new_coverage_1w_cmplog/minimized_union_corpus/symcts_afl/corpus /media/honululu/Data/cov_drcov_stb/cmplog_symcts_afl
# collect_coverage ./local_experiment_new_coverage_1w_cmplog/minimized_union_corpus/symcts_afl/crashes /media/honululu/Data/cov_drcov_stb/cmplog_symcts_afl

# collect_coverage ./local_experiment_new_coverage_1w_cmplog/union_symcts/ /media/honululu/Data/cov_drcov_stb/cmplog_symcts
# collect_coverage ./local_experiment_new_coverage_1w_cmplog/union_symcts_afl/ /media/honululu/Data/cov_drcov_stb/cmplog_symcts_afl
# collect_coverage ./local_experiment_new_coverage_1w_cmplog/union_symcc_afl/ /media/honululu/Data/cov_drcov_stb/cmplog_symcc_afl

# rm -rf ./cov_drcov_stb/symcts_latest
# collect_coverage ./sync/symcts_latest/corpus ./cov_drcov_stb/symcts_latest
# collect_coverage ./sync/symcts_latest/crashes ./cov_drcov_stb/symcts_latest


