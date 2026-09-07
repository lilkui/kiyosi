#include <catch2/catch_test_macros.hpp>
#include <catch2/catch_approx.hpp>
#include <chrono>
#include <limits>
#include <type_traits>
#include <kiyosi/kiyosi.hpp>

namespace {
kiyosi::date day(int year, unsigned month, unsigned day_number)
{
    return kiyosi::date{std::chrono::year{year} / std::chrono::month{month} / std::chrono::day{day_number}};
}
} // namespace

TEST_CASE("Deferred CPU instruments expose validated pricing paths")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry = day(2025, 7, 1);
    auto parameters = kiyosi::make_bsm_parameters(0.03, 0.01, 0.2);
    auto asset = kiyosi::make_asset_price(100.0);
    auto context = kiyosi::make_pricing_context(*parameters, *asset, valuation);
    REQUIRE(context.has_value());

    auto asian = kiyosi::make_geometric_average_option(kiyosi::option_type::call, 100.0, valuation, expiry);
    REQUIRE(asian.has_value());
    auto asian_result = kiyosi::GeometricAverageAsianEngine{}.price(*asian, *context);
    REQUIRE(asian_result.has_value());
    REQUIRE(asian_result->has(kiyosi::risk_measure::price));
    REQUIRE(*asian_result->get(kiyosi::risk_measure::price) > 0.0);

    auto note = kiyosi::make_binary_snowball_option(
        {0.1}, 0.05, 100.0, {110.0}, 100.0, 60.0, {expiry},
        kiyosi::barrier_touch_status::none, 1.0, valuation, expiry);
    REQUIRE(note);
    kiyosi::MonteCarloBinarySnowballEngine engine{{128, 7}};
    auto note_result = engine.price(*note, *context);
    REQUIRE(note_result.has_value());
    REQUIRE(note_result->has(kiyosi::risk_measure::price));
}

TEST_CASE("Shared numerical analytics and immutable coupon replacement")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry = day(2025, 7, 1);
    auto parameters = kiyosi::make_bsm_parameters(0.03, 0.01, 0.2);
    auto asset = kiyosi::make_asset_price(100.0);
    auto context = kiyosi::make_pricing_context(*parameters, *asset, valuation);
    REQUIRE(context.has_value());
    auto option = kiyosi::make_european_call(100.0, expiry);
    REQUIRE(option.has_value());
    auto analytics = kiyosi::numerical_analytics(kiyosi::BinomialEuropeanEngine{64}, *option, *context);
    REQUIRE(analytics.has_value());
    CHECK(analytics->has(kiyosi::risk_measure::speed));
    CHECK(analytics->has(kiyosi::risk_measure::rho));
    kiyosi::NumericalAnalyticsEngine<kiyosi::BinomialEuropeanEngine> shared{kiyosi::BinomialEuropeanEngine{64}};
    auto shared_result = shared.price(*option, *context);
    REQUIRE(shared_result.has_value());
    CHECK(shared_result->has(kiyosi::risk_measure::vega));

    const auto note = kiyosi::make_snowball_option(
        {0.1}, 0.05, 100.0, 60.0, {110.0}, 100.0, 60.0, {expiry},
        kiyosi::observation_frequency::at_expiry, kiyosi::barrier_touch_status::none,
        1.0, valuation, expiry);
    REQUIRE(note);
    const auto replaced = note->with_coupon_rate(0.08);
    REQUIRE(replaced);
    CHECK(note->maturity_coupon_rate() == 0.05);
    CHECK(replaced->maturity_coupon_rate() == 0.08);
}

TEST_CASE("Effective dates schedules and SSE calendar semantics")
{
    const auto effective = day(2025, 1, 3);
    const auto expiry = day(2025, 2, 3);
    const auto option = kiyosi::make_european_call(100.0, effective, expiry);
    REQUIRE(option);
    CHECK(option->effective() == effective);
    CHECK_FALSE(kiyosi::make_european_call(100.0, expiry, effective));

    const auto fixed = kiyosi::make_fixed_interval_schedule(effective, day(2025, 1, 7), 1);
    REQUIRE(fixed);
    CHECK(fixed->dates() == std::vector<kiyosi::date>{day(2025, 1, 6), day(2025, 1, 7)});
    CHECK(kiyosi::make_fixed_interval_schedule(effective, day(2025, 1, 5), 10)->empty());

    const auto monthly = kiyosi::make_monthly_schedule(day(2025, 1, 2), day(2025, 4, 2), 2);
    REQUIRE(monthly);
    CHECK(monthly->dates() == std::vector<kiyosi::date>{day(2025, 3, 3), day(2025, 4, 2)});
    CHECK_FALSE(kiyosi::make_monthly_schedule(effective, expiry, 0));

    const auto sse = kiyosi::sse_calendar();
    CHECK(sse.annual_trading_days() == 243);
    CHECK_FALSE(sse.is_trading_day(day(1991, 2, 15)));
    CHECK_FALSE(sse.is_trading_day(day(2030, 9, 12)));
    CHECK_FALSE(sse.is_trading_day(day(2031, 1, 4)));
    CHECK(sse.is_trading_day(day(2031, 1, 2)));
}

TEST_CASE("Structured factories enforce validation and signed coupon replacement")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    static_assert(!std::is_constructible_v<kiyosi::Accumulator, double, double, double, double, double,
                                           kiyosi::date, kiyosi::date>);
    static_assert(!std::is_constructible_v<kiyosi::BinarySnowballOption, std::vector<double>, double, double,
                                           std::vector<double>, double, double, std::vector<kiyosi::date>,
                                           kiyosi::barrier_touch_status, double, kiyosi::date, kiyosi::date>);

    CHECK(kiyosi::make_accumulator(100.0, 110.0, 1.0, 2.0, 0.0, expiry, effective).error().category ==
          kiyosi::error_category::invalid_schedule);
    const auto note = kiyosi::make_snowball_option(
        {-0.1}, -0.05, 100.0, 60.0, {110.0}, 100.0, 60.0, {expiry},
        kiyosi::observation_frequency::daily, kiyosi::barrier_touch_status::none,
        1.0, effective, expiry);
    REQUIRE(note);
    const auto replaced = note->with_coupon_rate(-0.08);
    REQUIRE(replaced);
    CHECK(replaced->maturity_coupon_rate() == -0.08);
    CHECK_FALSE(note->with_coupon_rate(std::numeric_limits<double>::infinity()));
    CHECK(kiyosi::make_snowball_option(
              {0.1}, 0.05, 100.0, 60.0, {110.0}, 100.0, 60.0,
              {expiry, effective}, kiyosi::observation_frequency::daily,
              kiyosi::barrier_touch_status::none, 1.0, effective, expiry)
              .error()
              .category == kiyosi::error_category::invalid_schedule);
}

TEST_CASE("Structured expiry settlement applies state and final observations")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto market = [&](double spot) {
        return *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.0, 0.0, 0.2),
                                             *kiyosi::make_asset_price(spot), expiry);
    };
    const auto price = [&](const auto& note, double spot) {
        const auto result = kiyosi::MonteCarloStructuredEngine<std::remove_cvref_t<decltype(note)>>{{32, 7}}.price(note, market(spot));
        REQUIRE(result);
        return *result->get(kiyosi::risk_measure::price);
    };

    const auto phoenix = *kiyosi::make_phoenix_option(
        0.08, 100.0, 80.0, {100.0}, {90.0}, 100.0, 60.0, {expiry},
        kiyosi::observation_frequency::at_expiry, kiyosi::barrier_touch_status::none,
        1.0, effective, expiry);
    CHECK(price(phoenix, 100.0) == Catch::Approx(9.0));
    CHECK(price(phoenix, 80.0) == Catch::Approx(1.0));
    const auto phoenix_up = *kiyosi::make_phoenix_option(
        0.08, 100.0, 80.0, {100.0}, {90.0}, 100.0, 60.0, {expiry},
        kiyosi::observation_frequency::daily, kiyosi::barrier_touch_status::up,
        1.0, effective, expiry);
    CHECK(price(phoenix_up, 100.0) == 0.0);

    const auto snowball = *kiyosi::make_snowball_option(
        {0.10}, 0.05, 100.0, 80.0, {100.0}, 100.0, 60.0, {expiry},
        kiyosi::observation_frequency::at_expiry, kiyosi::barrier_touch_status::none,
        1.0, effective, expiry);
    CHECK(price(snowball, 100.0) == Catch::Approx(1.10));
    CHECK(price(snowball, 80.0) == Catch::Approx(1.05));
    CHECK(price(snowball, 59.0) == Catch::Approx(0.60));
    const auto snowball_down = *kiyosi::make_snowball_option(
        {0.10}, 0.05, 100.0, 80.0, {110.0}, 100.0, 60.0, {expiry},
        kiyosi::observation_frequency::daily, kiyosi::barrier_touch_status::down,
        1.0, effective, expiry);
    CHECK(price(snowball_down, 70.0) == Catch::Approx(0.70));

    const auto binary = *kiyosi::make_binary_snowball_option(
        {0.10}, 0.05, 100.0, {100.0}, 100.0, 60.0, {expiry},
        kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    CHECK(price(binary, 100.0) == Catch::Approx(1.10));
    CHECK(price(binary, 90.0) == Catch::Approx(1.05));

    const auto ternary = *kiyosi::make_ternary_snowball_option(
        {0.10}, 0.05, 0.02, 100.0, 80.0, {100.0}, 100.0, 60.0, {expiry},
        kiyosi::observation_frequency::at_expiry, kiyosi::barrier_touch_status::none,
        1.0, effective, expiry);
    CHECK(price(ternary, 100.0) == Catch::Approx(1.10));
    CHECK(price(ternary, 80.0) == Catch::Approx(1.05));
    CHECK(price(ternary, 79.0) == Catch::Approx(1.02));

    const auto accumulator = *kiyosi::make_accumulator(100.0, 110.0, 1.0, 2.0, 3.0, effective, expiry);
    CHECK(price(accumulator, 90.0) == Catch::Approx(-30.0));
}

TEST_CASE("Structured Monte Carlo resolves only future observation events")
{
    const auto effective = day(2025, 1, 1);
    const auto valuation = day(2025, 7, 1);
    const auto expiry = day(2026, 1, 1);
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.0, 0.0, 1e-12), *kiyosi::make_asset_price(100.0), valuation);
    const auto note = *kiyosi::make_binary_snowball_option(
        {10.0, 0.10}, 0.05, 100.0, {90.0, 99.0}, 100.0, 60.0,
        {valuation, expiry}, kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    const kiyosi::MonteCarloBinarySnowballEngine engine{{32, 7}};
    const auto first = engine.price(note, context);
    const auto second = engine.price(note, context);
    REQUIRE(first);
    REQUIRE(second);
    CHECK(*first->get(kiyosi::risk_measure::price) == *second->get(kiyosi::risk_measure::price));
    CHECK(*first->get(kiyosi::risk_measure::price) == Catch::Approx(1.10).margin(1e-10));
}
