#include <catch2/catch_test_macros.hpp>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

using kiyosi::test::day;

TEST_CASE("Autocallable engines share domain validation precedence", "[autocallable-validation]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2025, 1, 4);
    const FiniteDifferenceAutocallableEngine finite_difference{32, 2};
    const MonteCarloAutocallableEngine monte_carlo{2, 7};
    for (const bool known_history : {false, true}) {
        const auto note = *make_binary_snowball_option({.maturity_coupon_rate = 0.05,
                                                        .barrier_state = known_history ? std::optional{AutocallableBarrierState::none} : std::nullopt,
                                                        .effective_date = start,
                                                        .expiry_date = end,
                                                        .observations = {{day(2025, 1, 2), 110.0, 0.08}}});
        for (const auto valuation : {start, day(2025, 1, 3), day(2025, 1, 5)}) {
            const auto context = *make_pricing_context(*make_bsm_parameters(0.03, 0.0, 0.2), 100.0, valuation);
            const auto expected = valuation > end                                 ? ErrorCategory::invalid_time_range
                                  : !known_history && valuation > day(2025, 1, 2) ? ErrorCategory::invalid_parameter
                                                                                  : ErrorCategory::invalid_date;
            const auto check = [&](const auto& engine) {
                const auto price = engine.price(note, context);
                REQUIRE_FALSE(price);
                CHECK(price.error().category == expected);
                const auto greeks = engine.price_with_greeks(note, context, {Greek::delta});
                REQUIRE_FALSE(greeks);
                CHECK(greeks.error().category == expected);
                const auto volatility = implied_volatility(engine, note, context, 1.0);
                REQUIRE_FALSE(volatility);
                CHECK(volatility.error().category == expected);
            };
            check(finite_difference);
            check(monte_carlo);
        }
    }
}
