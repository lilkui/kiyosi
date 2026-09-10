#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>
#include <array>
#include <chrono>
#include <limits>
#include <string>
#include <type_traits>
#include <vector>
#include <kiyosi/kiyosi.hpp>
#include "support/common.hpp"

namespace {

using kiyosi::test::day;
using kiyosi::test::risk_value;

TEST_CASE("Analytic European engine returns reviewed call value and Greeks")
{
    using Catch::Matchers::WithinAbs;

    const auto valuation = day(2025, 1, 6);
    const auto option = *kiyosi::make_european_call(100.0, valuation + std::chrono::days{365});
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(100.0), valuation);

    const auto result = kiyosi::AnalyticEuropeanEngine{}.price(option, context);
    REQUIRE(result.has_value());
    CHECK_THAT(risk_value(*result, kiyosi::risk_measure::price), WithinAbs(13.151137, 1e-6));
    CHECK_THAT(risk_value(*result, kiyosi::risk_measure::delta), WithinAbs(0.592749, 1e-6));
    CHECK_THAT(risk_value(*result, kiyosi::risk_measure::gamma), WithinAbs(0.012761, 1e-6));
    CHECK_THAT(risk_value(*result, kiyosi::risk_measure::speed), WithinAbs(-0.000234, 1e-6));
    CHECK_THAT(risk_value(*result, kiyosi::risk_measure::theta), WithinAbs(-0.019163, 1e-6));
    CHECK_THAT(risk_value(*result, kiyosi::risk_measure::charm), WithinAbs(-0.000115, 1e-6));
    CHECK_THAT(risk_value(*result, kiyosi::risk_measure::color), WithinAbs(0.000019, 1e-6));
    CHECK_THAT(risk_value(*result, kiyosi::risk_measure::vega), WithinAbs(0.382821, 1e-6));
    CHECK_THAT(risk_value(*result, kiyosi::risk_measure::vanna), WithinAbs(0.000638, 1e-6));
    CHECK_THAT(risk_value(*result, kiyosi::risk_measure::zomma), WithinAbs(-0.000431, 1e-6));
    CHECK_THAT(risk_value(*result, kiyosi::risk_measure::rho), WithinAbs(0.461238, 1e-6));
}

TEST_CASE("Analytic European calls and puts obey BSM identities")
{
    using Catch::Matchers::WithinAbs;

    const auto valuation = day(2025, 1, 6);
    const auto expiry = valuation + std::chrono::days{365};
    const auto call = *kiyosi::make_european_call(100.0, expiry);
    const auto put = *kiyosi::make_european_put(100.0, expiry);
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(100.0), valuation);
    const kiyosi::AnalyticEuropeanEngine engine;

    const auto call_result = *engine.price(call, context);
    const auto put_result = *engine.price(put, context);
    CHECK_THAT(risk_value(put_result, kiyosi::risk_measure::price), WithinAbs(10.225098, 1e-6));
    CHECK_THAT(risk_value(call_result, kiyosi::risk_measure::price) - risk_value(put_result, kiyosi::risk_measure::price),
               WithinAbs(100.0 * std::exp(-0.01) - 100.0 * std::exp(-0.04), 1e-12));
    CHECK_THAT(risk_value(call_result, kiyosi::risk_measure::delta) - risk_value(put_result, kiyosi::risk_measure::delta), WithinAbs(std::exp(-0.01), 1e-12));
    CHECK_THAT(risk_value(call_result, kiyosi::risk_measure::gamma), WithinAbs(risk_value(put_result, kiyosi::risk_measure::gamma), 1e-12));
    CHECK_THAT(risk_value(call_result, kiyosi::risk_measure::speed), WithinAbs(risk_value(put_result, kiyosi::risk_measure::speed), 1e-12));
    CHECK_THAT(risk_value(call_result, kiyosi::risk_measure::color), WithinAbs(risk_value(put_result, kiyosi::risk_measure::color), 1e-12));
    CHECK_THAT(risk_value(call_result, kiyosi::risk_measure::vega), WithinAbs(risk_value(put_result, kiyosi::risk_measure::vega), 1e-12));
    CHECK_THAT(risk_value(call_result, kiyosi::risk_measure::vanna), WithinAbs(risk_value(put_result, kiyosi::risk_measure::vanna), 1e-12));
    CHECK_THAT(risk_value(call_result, kiyosi::risk_measure::zomma), WithinAbs(risk_value(put_result, kiyosi::risk_measure::zomma), 1e-12));
    CHECK_THAT(risk_value(put_result, kiyosi::risk_measure::theta), WithinAbs(-0.011346, 1e-6));
    CHECK_THAT(risk_value(put_result, kiyosi::risk_measure::charm), WithinAbs(-0.000142, 1e-6));
    CHECK_THAT(risk_value(put_result, kiyosi::risk_measure::rho), WithinAbs(-0.499552, 1e-6));
}

TEST_CASE("Analytic European engine remains finite one day before expiry")
{
    const auto valuation = day(2025, 1, 6);
    const auto option = *kiyosi::make_european_call(100.0, valuation + std::chrono::days{1});
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(100.0), valuation);

    const auto result = kiyosi::AnalyticEuropeanEngine{}.price(option, context);
    REQUIRE(result.has_value());
    for (const auto& item : result->values) {
        REQUIRE(item.has_value());
        const double value = *item;
        CHECK(std::isfinite(value));
    }
}

TEST_CASE("Analytic European engine returns intrinsic value and zero Greeks at expiry")
{
    const auto expiry = day(2026, 1, 6);
    const auto call = *kiyosi::make_european_call(100.0, expiry);
    const auto put = *kiyosi::make_european_put(100.0, expiry);
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto call_context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(110.0), expiry);
    const auto put_context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(90.0), expiry);
    const kiyosi::AnalyticEuropeanEngine engine;

    const auto call_result = *engine.price(call, call_context);
    const auto put_result = *engine.price(put, put_context);
    REQUIRE(risk_value(call_result, kiyosi::risk_measure::price) == 10.0);
    REQUIRE(risk_value(put_result, kiyosi::risk_measure::price) == 10.0);
    CHECK_FALSE(call_result.has(kiyosi::risk_measure::delta));
    CHECK_FALSE(call_result.has(kiyosi::risk_measure::gamma));
    CHECK_FALSE(call_result.has(kiyosi::risk_measure::theta));
}

TEST_CASE("Analytic European engine reports pricing and implied-volatility failures")
{
    using Catch::Matchers::WithinAbs;

    const auto valuation = day(2025, 1, 6);
    const auto expiry = valuation + std::chrono::days{365};
    const auto option = *kiyosi::make_european_call(100.0, expiry);
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(100.0), valuation);
    const kiyosi::AnalyticEuropeanEngine engine;

    const auto implied = engine.implied_volatility(option, context, 13.151137);
    REQUIRE(implied.has_value());
    CHECK_THAT(*implied, WithinAbs(0.3, 1e-7));

    const auto invalid_bracket = engine.implied_volatility(
        option, context, 13.151137, kiyosi::ImpliedVolatilitySettings{1.0, 0.1});
    REQUIRE_FALSE(invalid_bracket.has_value());
    CHECK(invalid_bracket.error().category == kiyosi::error_category::invalid_parameter);

    const auto unbracketed = engine.implied_volatility(option, context, 95.0);
    REQUIRE_FALSE(unbracketed.has_value());
    CHECK(unbracketed.error().category == kiyosi::error_category::unbracketed_volatility);

    const auto non_finite_price = engine.implied_volatility(
        option, context, std::numeric_limits<double>::quiet_NaN());
    REQUIRE_FALSE(non_finite_price.has_value());
    CHECK(non_finite_price.error().category == kiyosi::error_category::invalid_parameter);

    const auto unconverged = engine.implied_volatility(
        option, context, 13.151137,
        kiyosi::ImpliedVolatilitySettings{.tolerance = 1e-15, .max_iterations = 1});
    REQUIRE_FALSE(unconverged.has_value());
    CHECK(unconverged.error().category == kiyosi::error_category::solver_non_convergence);

    const auto expired_context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(100.0), expiry);
    REQUIRE_FALSE(engine.implied_volatility(option, expired_context, 0.0).has_value());

    const auto stale_context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(100.0), expiry + std::chrono::days{1});
    const auto invalid_expiry = engine.price(option, stale_context);
    REQUIRE_FALSE(invalid_expiry.has_value());
    CHECK(invalid_expiry.error().category == kiyosi::error_category::invalid_expiry);

    const auto extreme_parameters = *kiyosi::make_bsm_parameters(-1000.0, 0.0, 0.3);
    const auto extreme_context = *kiyosi::make_pricing_context(extreme_parameters, *kiyosi::make_asset_price(100.0), valuation);
    const auto long_option = *kiyosi::make_european_put(
        100.0, valuation + std::chrono::days{36500});
    const auto non_finite_result = engine.price(long_option, extreme_context);
    REQUIRE_FALSE(non_finite_result.has_value());
    CHECK(non_finite_result.error().category == kiyosi::error_category::invalid_result);

    const auto non_finite_solver = engine.implied_volatility(long_option, extreme_context, 1.0);
    REQUIRE_FALSE(non_finite_solver.has_value());
    CHECK(non_finite_solver.error().category == kiyosi::error_category::solver_non_finite);
}

TEST_CASE("Analytic European engine remains finite at near-zero volatility")
{
    const auto valuation = day(2025, 1, 6);
    const auto option = *kiyosi::make_european_call(100.0, valuation + std::chrono::days{1});
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 1e-12);
    const auto context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(100.0), valuation);

    const auto result = kiyosi::AnalyticEuropeanEngine{}.price(option, context);
    REQUIRE(result.has_value());
    CHECK(std::isfinite(risk_value(*result, kiyosi::risk_measure::price)));
    CHECK(std::isfinite(risk_value(*result, kiyosi::risk_measure::delta)));
    CHECK_FALSE(result->has(kiyosi::risk_measure::gamma));
}

TEST_CASE("Analytic European implied volatility enforces arbitrage bounds and handles moneyness")
{
    using Catch::Matchers::WithinAbs;

    const auto valuation = day(2025, 1, 6);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.02, 0.01, 0.35);
    const kiyosi::AnalyticEuropeanEngine engine;

    const auto call = *kiyosi::make_european_call(100.0, expiry);
    const auto call_context = *kiyosi::make_pricing_context(
        parameters, *kiyosi::make_asset_price(100.0), valuation);
    const auto call_price = *engine.price(call, call_context);
    const auto call_implied = engine.implied_volatility(call, call_context,
                                                        risk_value(call_price, kiyosi::risk_measure::price));
    REQUIRE(call_implied.has_value());
    CHECK_THAT(*call_implied, WithinAbs(0.35, 1e-7));

    const auto deep_in_the_money = *kiyosi::make_european_call(20.0, expiry);
    const auto deep_itm_price = *engine.price(deep_in_the_money, call_context);
    const auto deep_itm_implied = engine.implied_volatility(deep_in_the_money, call_context,
                                                            risk_value(deep_itm_price, kiyosi::risk_measure::price));
    REQUIRE(deep_itm_implied.has_value());
    CHECK_THAT(*deep_itm_implied, WithinAbs(0.35, 1e-6));

    const auto deep_out_of_the_money = *kiyosi::make_european_call(180.0, expiry);
    const auto deep_otm_price = *engine.price(deep_out_of_the_money, call_context);
    const auto deep_otm_implied = engine.implied_volatility(deep_out_of_the_money, call_context,
                                                            risk_value(deep_otm_price, kiyosi::risk_measure::price));
    REQUIRE(deep_otm_implied.has_value());
    CHECK_THAT(*deep_otm_implied, WithinAbs(0.35, 1e-6));

    const auto invalid_quote = engine.implied_volatility(call, call_context, 0.01);
    REQUIRE_FALSE(invalid_quote.has_value());
    CHECK(invalid_quote.error().category == kiyosi::error_category::invalid_quote);

    const auto negative_quote = engine.implied_volatility(call, call_context, -1.0);
    REQUIRE_FALSE(negative_quote.has_value());
    CHECK(negative_quote.error().category == kiyosi::error_category::invalid_quote);

    const auto boundary_put = *kiyosi::make_european_put(120.0, expiry);
    const auto zero_carry = *kiyosi::make_bsm_parameters(0.0, 0.0, 0.35);
    const auto zero_carry_context = *kiyosi::make_pricing_context(
        zero_carry, *kiyosi::make_asset_price(100.0), valuation);
    const auto boundary = engine.implied_volatility(boundary_put, zero_carry_context, 20.0);
    REQUIRE(boundary.has_value());
    CHECK(*boundary == kiyosi::ImpliedVolatilitySettings{}.lower_bound);
}

TEST_CASE("Analytic European engine remains finite in deep tails")
{
    const auto valuation = day(2025, 1, 6);
    const auto option = *kiyosi::make_european_call(1'000'000.0, valuation + std::chrono::days{1});
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(100.0), valuation);

    const auto result = kiyosi::AnalyticEuropeanEngine{}.price(option, context);
    REQUIRE(result.has_value());
    for (const auto& item : result->values) {
        REQUIRE(item.has_value());
        const double value = *item;
        CHECK(std::isfinite(value));
    }
}

TEST_CASE("Analytic European engine remains finite for a short-dated low-volatility option")
{
    const auto valuation = day(2025, 1, 6);
    const auto option = *kiyosi::make_european_put(200.0, valuation + std::chrono::days{1});
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.05);
    const auto context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(100.0), valuation);

    const auto result = kiyosi::AnalyticEuropeanEngine{}.price(option, context);
    REQUIRE(result.has_value());
    CHECK(std::isfinite(risk_value(*result, kiyosi::risk_measure::price)));
    CHECK(std::isfinite(risk_value(*result, kiyosi::risk_measure::delta)));
}

}
