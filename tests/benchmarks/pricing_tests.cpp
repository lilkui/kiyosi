#include <vector>

#include <catch2/catch_test_macros.hpp>
#include <kiyosi/market/bsm_parameters.hpp>

#include "../../benchmarks/pricing.hpp"
#include "support/common.hpp"

namespace {
class PricingReporter final : public benchmark::BenchmarkReporter {
public:
    std::vector<Run> runs;
    bool ReportContext(const Context&) override { return true; }
    void ReportRuns(const std::vector<Run>& reports) override
    {
        runs.insert(runs.end(), reports.begin(), reports.end());
    }
};

struct FailingEngine {
    int* calls;
    int failure_call;
    kiyosi::Result<double> price(int, const kiyosi::PricingContext&) const
    {
        if (++*calls == failure_call)
            return std::unexpected(kiyosi::Error{kiyosi::ErrorCategory::invalid_result, "pricing failed"});
        return 42.0;
    }
};

TEST_CASE("Pricing benchmarks report warmup and measured failures", "[benchmarks]")
{
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, kiyosi::test::day(2025, 1, 1));
    for (const int failure_call : {0, 1, 2}) {
        CAPTURE(failure_call);
        int calls = 0;
        benchmark::ClearRegisteredBenchmarks();
        kiyosi::benchmark_support::register_price("test/pricing", 0, FailingEngine{&calls, failure_call}, context)
            ->Iterations(3);
        PricingReporter reporter;
        const auto matched = benchmark::RunSpecifiedBenchmarks(&reporter, "test/pricing");
        benchmark::ClearRegisteredBenchmarks();
        REQUIRE(matched == 1);
        REQUIRE(reporter.runs.size() == 1);
        const auto& run = reporter.runs.front();
        if (failure_call == 0) {
            CHECK(calls == 4);
            CHECK(run.skipped == benchmark::internal::NotSkipped);
            CHECK(run.counters.at("price") == 42.0);
        } else {
            CHECK(calls == failure_call);
            CHECK(run.skipped == benchmark::internal::SkippedWithError);
            CHECK(run.skip_message == "pricing failed");
        }
    }
}
} // namespace
