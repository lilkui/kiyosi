#include <catch2/catch_test_macros.hpp>
#include <chrono>
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

    auto note = kiyosi::BinarySnowballOption{{0.1}, 0.05, 100.0, {110.0}, 100.0, 60.0, {expiry}, kiyosi::barrier_touch_status::none, 1.0, valuation, expiry};
    kiyosi::MonteCarloBinarySnowballEngine engine{{128, 7}};
    auto note_result = engine.price(note, *context);
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

    const kiyosi::SnowballOption note{{0.1}, 0.05, 100.0, 60.0, {110.0}, 100.0, 60.0,
                                      {expiry}, kiyosi::observation_frequency::at_expiry,
                                      kiyosi::barrier_touch_status::none, 1.0, valuation, expiry};
    const auto replaced = note.with_coupon_rate(0.08);
    CHECK(note.maturity_coupon_rate() == 0.05);
    CHECK(replaced.maturity_coupon_rate() == 0.08);
}
