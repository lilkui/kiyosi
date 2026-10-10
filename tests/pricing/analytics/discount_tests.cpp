#include <cmath>
#include <numbers>

#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>
#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

TEST_CASE("Prices survive underflowing discount factors", "[audit-fixes]")
{
    using namespace kiyosi;
    const auto start = test::day(2025, 1, 1);
    const auto end = test::day(2026, 1, 1);
    constexpr double amount = 1e300;
    for (const double rate : {710.0, 740.0, 750.0}) {
        const auto context = *make_pricing_context(*make_bsm_parameters(rate, rate, 0.2), amount, start);
        const double discounted = std::exp(std::log(amount) - rate);
        for (const auto type : {OptionType::call, OptionType::put}) {
            const double sign = type == OptionType::call ? 1.0 : -1.0;
            const auto check = [&](const auto& option, double expected, const auto& analytic, const auto& quadrature) {
                for (const auto& result : {analytic.price(option, context), quadrature.price(option, context)}) {
                    CAPTURE(rate, type);
                    REQUIRE(result);
                    CHECK(*result == Catch::Approx(expected).epsilon(1e-10).margin(0.0));
                }
            };
            check(*make_cash_or_nothing_option(type, amount, amount, start, end),
                  discounted * 0.5 * std::erfc(sign * 0.1 / std::numbers::sqrt2),
                  AnalyticDigitalEngine{}, QuadratureDigitalEngine{});
            check(*make_asset_or_nothing_option(type, amount, start, end),
                  discounted * 0.5 * std::erfc(-sign * 0.1 / std::numbers::sqrt2),
                  AnalyticDigitalEngine{}, QuadratureDigitalEngine{});
            check(*make_european_option(type, amount, start, end),
                  discounted * std::erf(0.1 / std::numbers::sqrt2),
                  AnalyticVanillaEngine{}, QuadratureVanillaEngine{});
        }
    }
}
