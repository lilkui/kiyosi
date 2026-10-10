#include <cmath>
#include <numbers>

#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>
#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {
void check_monte_carlo_discount_scaling(kiyosi::MonteCarloBackend backend)
{
    using namespace kiyosi;
    const auto start = test::day(2025, 1, 1);
    const auto end = test::day(2026, 1, 1);
    const MonteCarloVanillaEngine engine{10'000, 2, 42, backend};
    const auto unit_context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, 0.2), 1.0, start);
    for (const auto type : {OptionType::call, OptionType::put}) {
        const auto unit = engine.price(*make_european_option(type, 1.0, start, end), unit_context);
        REQUIRE(unit);
        for (const double rate : {710.0, 740.0, 750.0, -750.0}) {
            CAPTURE(backend, type, rate);
            const double amount = rate > 0.0 ? 1e300 : 1e-300;
            const auto context = *make_pricing_context(*make_bsm_parameters(rate, rate, 0.2), amount, start);
            const auto price = engine.price(*make_european_option(type, amount, start, end), context);
            REQUIRE(price);
            CHECK(*price == Catch::Approx(*unit * std::exp(std::log(amount) - rate)).epsilon(2e-12).margin(0.0));
        }
    }
}
} // namespace

TEST_CASE("European Monte Carlo preserves prices across extreme discount scales", "[audit-fixes]")
{
    check_monte_carlo_discount_scaling(kiyosi::MonteCarloBackend::cpu);
}

#if KIYOSI_HAS_CUDA
TEST_CASE("CUDA European Monte Carlo preserves prices across extreme discount scales", "[cuda][audit-fixes]")
{
    check_monte_carlo_discount_scaling(kiyosi::MonteCarloBackend::cuda);
}
#endif

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
