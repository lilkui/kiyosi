#include <catch2/catch_test_macros.hpp>

#include <kiyosi/kiyosi.hpp>

#include "pricing/detail/autocallable_pricer.hpp"
#include "support/common.hpp"

using kiyosi::test::day;

TEST_CASE("Autocallable engines share domain validation precedence", "[autocallable-validation]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2025, 1, 4);
    const auto finite_difference = make_finite_difference_autocallable_engine({32, 2}).value();
    const auto monte_carlo = make_monte_carlo_autocallable_engine({2, 7}).value();
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

TEST_CASE("Autocallable requests recheck valuation and history after validation", "[autocallable-validation]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 6), observation = day(2025, 1, 8), end = day(2025, 1, 10);
    const auto note = *make_binary_snowball_option({.maturity_coupon_rate = 0.05,
                                                    .barrier_state = std::nullopt,
                                                    .effective_date = start,
                                                    .expiry_date = end,
                                                    .observations = {{observation, 110.0, 0.08}}});
    const auto parameters = *make_bsm_parameters(0.03, 0.0, 0.2);
    for (const bool trading_days : {true, false}) {
        CAPTURE(trading_days);
        const auto calendar = trading_days ? weekdays_calendar() : *make_trading_calendar([](Date) { return false; }, 252);
        detail::AutocallableContract<BinarySnowballOption> contract{note, calendar};
        const auto check = [&](Date valuation, std::optional<ErrorCategory> expected) {
            CAPTURE(valuation);
            const auto context = *make_pricing_context(parameters, 100.0, valuation, calendar);
            const auto valid = contract.validate(context);
            if (expected) {
                REQUIRE_FALSE(valid);
                CHECK(valid.error().category == *expected);
            } else {
                REQUIRE(valid);
            }
        };
        const std::optional<ErrorCategory> initial_error = trading_days ? std::nullopt : std::optional{ErrorCategory::invalid_date};
        check(day(2025, 1, 5), ErrorCategory::invalid_time_range);
        check(start, initial_error);
        check(day(2025, 1, 9), trading_days ? ErrorCategory::invalid_parameter : ErrorCategory::invalid_date);
        check(day(2025, 1, 11), ErrorCategory::invalid_time_range);
        check(start, initial_error);
    }
}
