#include <cmath>
#include <numbers>

#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>
#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {
using namespace kiyosi;
using kiyosi::test::day;

TEST_CASE("Deep in-the-money digital prices survive overflowing discount factors", "[audit-fixes]")
{
    const auto start = day(2025, 1, 1);
    const auto end = day(2026, 1, 1);
    constexpr double settlement = 1e-300;
    const auto context = *make_pricing_context(*make_bsm_parameters(-710.0, -710.0, 0.2), settlement, start);
    const double expected = std::exp(std::log(settlement) + 710.0);
    for (const auto type : {OptionType::call, OptionType::put}) {
        const double strike = type == OptionType::call ? 1e-304 : 1e-296;
        const auto check = [&](const auto& option) {
            for (const auto& result : {AnalyticDigitalEngine{}.price(option, context), QuadratureDigitalEngine{}.price(option, context)}) {
                CAPTURE(type);
                REQUIRE(result);
                CHECK(*result == Catch::Approx(expected).epsilon(1e-12));
            }
        };
        check(*make_cash_or_nothing_option(type, strike, settlement, start, end));
        check(*make_asset_or_nothing_option(type, strike, start, end));
    }
}

TEST_CASE("Digital prices survive overflowing discounted settlements", "[audit-fixes]")
{
    const auto start = day(2025, 1, 1);
    const auto end = day(2026, 1, 1);
    constexpr double settlement = 1.75e308;
    for (const auto type : {OptionType::call, OptionType::put}) {
        const double probability = 0.5 * std::erfc((type == OptionType::call ? -0.2 : 0.2) / std::numbers::sqrt2);
        for (const bool asset : {false, true}) {
            for (const bool overflow : {false, true}) {
                CAPTURE(type, asset, overflow);
                const double discount_rate = overflow ? -1.0 : -0.05;
                const auto parameters = asset ? *make_bsm_parameters(discount_rate + 0.02, discount_rate, 0.2)
                                              : *make_bsm_parameters(discount_rate, discount_rate - 0.06, 0.2);
                const double spot = asset ? settlement : 100.0;
                const auto context = *make_pricing_context(parameters, spot, start);
                const auto check = [&](const auto& engine, const auto& option) {
                    const auto result = engine.price(option, context);
                    if (overflow) {
                        REQUIRE_FALSE(result);
                        CHECK(result.error().category == ErrorCategory::invalid_result);
                    } else {
                        REQUIRE(result);
                        CHECK(*result == Catch::Approx(settlement * (std::exp(0.05) * probability)).epsilon(1e-10));
                    }
                };
                const auto check_engines = [&](const auto& option) {
                    check(AnalyticDigitalEngine{}, option);
                    check(QuadratureDigitalEngine{}, option);
                };
                if (asset)
                    check_engines(*make_asset_or_nothing_option(type, spot, start, end));
                else
                    check_engines(*make_cash_or_nothing_option(type, spot, settlement, start, end));
            }
        }
    }
}

} // namespace
