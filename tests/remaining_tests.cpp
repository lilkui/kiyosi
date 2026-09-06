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
