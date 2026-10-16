#include <kiyosi/pricing/implied.hpp>

#include <kiyosi/instruments/barrier/option.hpp>
#include <kiyosi/pricing/engines/barrier/analytic.hpp>
#include <kiyosi/pricing/engines/vanilla/binomial.hpp>

#include <catch2/catch_test_macros.hpp>

#include "support/common.hpp"

namespace {

TEST_CASE("Analytics headers preserve product hooks when included before engines", "[analytics-headers]")
{
    using namespace kiyosi;
    const auto start = test::day(2025, 1, 1), end = test::day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(1.0, 0.0, 1.0), 100.0, start);
    const auto option = *make_european_option(OptionType::call, 100.0, start, end);
    const CoxRossRubinsteinVanillaEngine engine{4};
    const auto quote = engine.price(option, context);
    REQUIRE(quote);
    // The default lower volatility lies outside this tree's admissible domain.
    const auto solved = implied_volatility(engine, option, context, *quote);
    REQUIRE(solved);
    CHECK(std::abs(*solved - 1.0) < 1e-6);

    const auto barrier = *make_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = 100.0, .barrier_type = BarrierType::up_and_out});
    const auto result = AnalyticBarrierEngine{}.price_with_greeks(barrier, context, true);
    REQUIRE(result);
    CHECK(result->price() == 0.0);
    for (const auto greek : {Greek::delta, Greek::gamma, Greek::theta, Greek::vega, Greek::rho})
        CHECK_FALSE(result->has(greek));
    const auto fixed = implied_volatility(AnalyticBarrierEngine{}, barrier, context, 0.0);
    REQUIRE_FALSE(fixed);
    CHECK(fixed.error().category == ErrorCategory::unsupported_operation);
}

} // namespace
