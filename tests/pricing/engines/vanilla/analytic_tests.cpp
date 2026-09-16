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

TEST_CASE("Analytic European calls and puts obey BSM identities")
{
    using Catch::Matchers::WithinAbs;

    const auto valuation = day(2025, 1, 6);
    const auto expiry = valuation + std::chrono::days{365};
    const auto call = *kiyosi::make_european_option(kiyosi::option_type::call, 100.0, valuation, expiry);
    const auto put = *kiyosi::make_european_option(kiyosi::option_type::put, 100.0, valuation, expiry);
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const kiyosi::AnalyticVanillaEngine engine;

    const auto call_result = *engine.price(call, context);
    const auto put_result = *engine.price(put, context);
    CHECK_THAT(risk_value(call_result, kiyosi::risk_measure::price) - risk_value(put_result, kiyosi::risk_measure::price),
               WithinAbs(100.0 * std::exp(-0.01) - 100.0 * std::exp(-0.04), 1e-12));
    CHECK_THAT(risk_value(call_result, kiyosi::risk_measure::delta) - risk_value(put_result, kiyosi::risk_measure::delta), WithinAbs(std::exp(-0.01), 1e-12));
    CHECK_THAT(risk_value(call_result, kiyosi::risk_measure::gamma), WithinAbs(risk_value(put_result, kiyosi::risk_measure::gamma), 1e-12));
    CHECK_THAT(risk_value(call_result, kiyosi::risk_measure::speed), WithinAbs(risk_value(put_result, kiyosi::risk_measure::speed), 1e-12));
    CHECK_THAT(risk_value(call_result, kiyosi::risk_measure::color), WithinAbs(risk_value(put_result, kiyosi::risk_measure::color), 1e-12));
    CHECK_THAT(risk_value(call_result, kiyosi::risk_measure::vega), WithinAbs(risk_value(put_result, kiyosi::risk_measure::vega), 1e-12));
    CHECK_THAT(risk_value(call_result, kiyosi::risk_measure::vanna), WithinAbs(risk_value(put_result, kiyosi::risk_measure::vanna), 1e-12));
    CHECK_THAT(risk_value(call_result, kiyosi::risk_measure::zomma), WithinAbs(risk_value(put_result, kiyosi::risk_measure::zomma), 1e-12));
}

TEST_CASE("Analytic European engine remains finite one day before expiry")
{
    const auto valuation = day(2025, 1, 6);
    const auto option = *kiyosi::make_european_option(
        kiyosi::option_type::call, 100.0, valuation, valuation + std::chrono::days{1});
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);

    const auto result = kiyosi::AnalyticVanillaEngine{}.price(option, context);
    REQUIRE(result.has_value());
    for (const auto& item : result->values_view()) {
        REQUIRE(item.has_value());
        const double value = *item;
        CHECK(std::isfinite(value));
    }
}

TEST_CASE("Analytic European engine returns intrinsic value and zero Greeks at expiry")
{
    const auto expiry = day(2026, 1, 6);
    const auto call = *kiyosi::make_european_option(kiyosi::option_type::call, 100.0, expiry, expiry);
    const auto put = *kiyosi::make_european_option(kiyosi::option_type::put, 100.0, expiry, expiry);
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto call_context = *kiyosi::make_pricing_context(parameters, 110.0, expiry);
    const auto put_context = *kiyosi::make_pricing_context(parameters, 90.0, expiry);
    const kiyosi::AnalyticVanillaEngine engine;

    const auto call_result = *engine.price(call, call_context);
    const auto put_result = *engine.price(put, put_context);
    REQUIRE(risk_value(call_result, kiyosi::risk_measure::price) == 10.0);
    REQUIRE(risk_value(put_result, kiyosi::risk_measure::price) == 10.0);
    CHECK_FALSE(call_result.has(kiyosi::risk_measure::delta));
    CHECK_FALSE(call_result.has(kiyosi::risk_measure::gamma));
    CHECK_FALSE(call_result.has(kiyosi::risk_measure::theta));
}

TEST_CASE("Analytic European implied volatility recovers market volatility")
{
    using Catch::Matchers::WithinAbs;

    const auto valuation = day(2025, 1, 6);
    const auto expiry = valuation + std::chrono::days{365};
    const auto option = *kiyosi::make_european_option(kiyosi::option_type::call, 100.0, valuation, expiry);
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const kiyosi::AnalyticVanillaEngine engine;

    const auto price = risk_value(*engine.price(option, context), kiyosi::risk_measure::price);
    const auto implied = kiyosi::implied_volatility(engine, option, context, price);
    REQUIRE(implied.has_value());
    CHECK_THAT(*implied, WithinAbs(0.3, 1e-7));
}

TEST_CASE("Analytic European implied volatility rejects invalid settings and prices")
{
    const auto valuation = day(2025, 1, 6);
    const auto expiry = valuation + std::chrono::days{365};
    const auto option = *kiyosi::make_european_option(kiyosi::option_type::call, 100.0, valuation, expiry);
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const kiyosi::AnalyticVanillaEngine engine;
    const auto price = risk_value(*engine.price(option, context), kiyosi::risk_measure::price);
    const auto invalid_bracket = kiyosi::implied_volatility(
        engine, option, context, price, kiyosi::ImpliedVolatilitySettings{1.0, 0.1});
    REQUIRE_FALSE(invalid_bracket.has_value());
    CHECK(invalid_bracket.error().category == kiyosi::error_category::invalid_parameter);

    const auto non_finite_price = kiyosi::implied_volatility(
        engine, option, context, std::numeric_limits<double>::quiet_NaN());
    REQUIRE_FALSE(non_finite_price.has_value());
    CHECK(non_finite_price.error().category == kiyosi::error_category::invalid_parameter);
}

TEST_CASE("Analytic European implied volatility reports solver failures")
{
    const auto valuation = day(2025, 1, 6);
    const auto expiry = valuation + std::chrono::days{365};
    const auto option = *kiyosi::make_european_option(kiyosi::option_type::call, 100.0, valuation, expiry);
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const kiyosi::AnalyticVanillaEngine engine;
    const auto price = risk_value(*engine.price(option, context), kiyosi::risk_measure::price);
    const auto unbracketed = kiyosi::implied_volatility(engine, option, context, 95.0);
    REQUIRE_FALSE(unbracketed.has_value());
    CHECK(unbracketed.error().category == kiyosi::error_category::unbracketed_volatility);

    const auto unconverged = kiyosi::implied_volatility(
        engine, option, context, price,
        kiyosi::ImpliedVolatilitySettings{.tolerance = 1e-15, .max_iterations = 1});
    REQUIRE_FALSE(unconverged.has_value());
    CHECK(unconverged.error().category == kiyosi::error_category::solver_non_convergence);
}

TEST_CASE("Analytic European engine reports invalid expiry")
{
    const auto valuation = day(2025, 1, 6);
    const auto expiry = valuation + std::chrono::days{365};
    const auto option = *kiyosi::make_european_option(kiyosi::option_type::call, 100.0, valuation, expiry);
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const kiyosi::AnalyticVanillaEngine engine;

    const auto stale_context = *kiyosi::make_pricing_context(parameters, 100.0, expiry + std::chrono::days{1});
    const auto invalid_expiry = engine.price(option, stale_context);
    REQUIRE_FALSE(invalid_expiry.has_value());
    CHECK(invalid_expiry.error().category == kiyosi::error_category::invalid_expiry);
}

TEST_CASE("Analytic European engine reports non-finite pricing and solver results")
{
    const auto valuation = day(2025, 1, 6);
    const kiyosi::AnalyticVanillaEngine engine;
    const auto extreme_parameters = *kiyosi::make_bsm_parameters(-1000.0, 0.0, 0.3);
    const auto extreme_context = *kiyosi::make_pricing_context(extreme_parameters, 100.0, valuation);
    const auto long_option = *kiyosi::make_european_option(
        kiyosi::option_type::put, 100.0, valuation, valuation + std::chrono::days{36500});
    const auto non_finite_result = engine.price(long_option, extreme_context);
    REQUIRE_FALSE(non_finite_result.has_value());
    CHECK(non_finite_result.error().category == kiyosi::error_category::invalid_result);

    const auto non_finite_solver = kiyosi::implied_volatility(
        engine, long_option, extreme_context, 1.0);
    REQUIRE_FALSE(non_finite_solver.has_value());
    CHECK(non_finite_solver.error().category == kiyosi::error_category::invalid_result);
}

TEST_CASE("Analytic European engine remains finite at near-zero volatility")
{
    const auto valuation = day(2025, 1, 6);
    const auto option = *kiyosi::make_european_option(
        kiyosi::option_type::call, 100.0, valuation, valuation + std::chrono::days{1});
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 1e-12);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);

    const auto result = kiyosi::AnalyticVanillaEngine{}.price(option, context);
    REQUIRE(result.has_value());
    CHECK(std::isfinite(risk_value(*result, kiyosi::risk_measure::price)));
    CHECK(std::isfinite(risk_value(*result, kiyosi::risk_measure::delta)));
    CHECK_FALSE(result->has(kiyosi::risk_measure::gamma));
}

TEST_CASE("Analytic European implied volatility recovers at-the-money volatility")
{
    using Catch::Matchers::WithinAbs;

    const auto valuation = day(2025, 1, 6);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.02, 0.01, 0.35);
    const kiyosi::AnalyticVanillaEngine engine;

    const auto call = *kiyosi::make_european_option(kiyosi::option_type::call, 100.0, valuation, expiry);
    const auto call_context = *kiyosi::make_pricing_context(
        parameters, 100.0, valuation);
    const auto call_price = *engine.price(call, call_context);
    const auto call_implied = kiyosi::implied_volatility(
        engine, call, call_context, risk_value(call_price, kiyosi::risk_measure::price));
    REQUIRE(call_implied.has_value());
    CHECK_THAT(*call_implied, WithinAbs(0.35, 1e-7));
}

TEST_CASE("Analytic European implied volatility recovers deep-in-the-money volatility")
{
    using Catch::Matchers::WithinAbs;
    const auto valuation = day(2025, 1, 6);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.02, 0.01, 0.35);
    const kiyosi::AnalyticVanillaEngine engine;
    const auto call_context = *kiyosi::make_pricing_context(
        parameters, 100.0, valuation);
    const auto deep_in_the_money = *kiyosi::make_european_option(
        kiyosi::option_type::call, 20.0, valuation, expiry);
    const auto deep_itm_price = *engine.price(deep_in_the_money, call_context);
    const auto deep_itm_implied = kiyosi::implied_volatility(
        engine, deep_in_the_money, call_context,
        risk_value(deep_itm_price, kiyosi::risk_measure::price));
    REQUIRE(deep_itm_implied.has_value());
    CHECK_THAT(*deep_itm_implied, WithinAbs(0.35, 1e-5));
}

TEST_CASE("Analytic European implied volatility recovers deep-out-of-the-money volatility")
{
    using Catch::Matchers::WithinAbs;
    const auto valuation = day(2025, 1, 6);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.02, 0.01, 0.35);
    const kiyosi::AnalyticVanillaEngine engine;
    const auto call_context = *kiyosi::make_pricing_context(
        parameters, 100.0, valuation);
    const auto deep_out_of_the_money = *kiyosi::make_european_option(
        kiyosi::option_type::call, 180.0, valuation, expiry);
    const auto deep_otm_price = *engine.price(deep_out_of_the_money, call_context);
    const auto deep_otm_implied = kiyosi::implied_volatility(
        engine, deep_out_of_the_money, call_context,
        risk_value(deep_otm_price, kiyosi::risk_measure::price));
    REQUIRE(deep_otm_implied.has_value());
    CHECK_THAT(*deep_otm_implied, WithinAbs(0.35, 1e-6));
}

TEST_CASE("Analytic European implied volatility reports unbracketed quotes")
{
    const auto valuation = day(2025, 1, 6);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.02, 0.01, 0.35);
    const kiyosi::AnalyticVanillaEngine engine;
    const auto call = *kiyosi::make_european_option(kiyosi::option_type::call, 100.0, valuation, expiry);
    const auto call_context = *kiyosi::make_pricing_context(
        parameters, 100.0, valuation);
    const auto invalid_quote = kiyosi::implied_volatility(engine, call, call_context, 0.01);
    REQUIRE_FALSE(invalid_quote.has_value());
    CHECK(invalid_quote.error().category == kiyosi::error_category::unbracketed_volatility);

    const auto negative_quote = kiyosi::implied_volatility(engine, call, call_context, -1.0);
    REQUIRE_FALSE(negative_quote.has_value());
    CHECK(negative_quote.error().category == kiyosi::error_category::unbracketed_volatility);

    const auto boundary_put = *kiyosi::make_european_option(
        kiyosi::option_type::put, 120.0, valuation, expiry);
    const auto zero_carry = *kiyosi::make_bsm_parameters(0.0, 0.0, 0.35);
    const auto zero_carry_context = *kiyosi::make_pricing_context(
        zero_carry, 100.0, valuation);
    const auto boundary = kiyosi::implied_volatility(
        engine, boundary_put, zero_carry_context, 20.0);
    REQUIRE(boundary.has_value());
    CHECK(*boundary == kiyosi::ImpliedVolatilitySettings{}.lower_bound);
}

TEST_CASE("Analytic European engine remains finite in deep tails")
{
    const auto valuation = day(2025, 1, 6);
    const auto option = *kiyosi::make_european_option(
        kiyosi::option_type::call, 1'000'000.0, valuation, valuation + std::chrono::days{1});
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);

    const auto result = kiyosi::AnalyticVanillaEngine{}.price(option, context);
    REQUIRE(result.has_value());
    for (const auto& item : result->values_view()) {
        REQUIRE(item.has_value());
        const double value = *item;
        CHECK(std::isfinite(value));
    }
}

TEST_CASE("Analytic European engine remains finite for a short-dated low-volatility option")
{
    const auto valuation = day(2025, 1, 6);
    const auto option = *kiyosi::make_european_option(
        kiyosi::option_type::put, 200.0, valuation, valuation + std::chrono::days{1});
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.05);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);

    const auto result = kiyosi::AnalyticVanillaEngine{}.price(option, context);
    REQUIRE(result.has_value());
    CHECK(std::isfinite(risk_value(*result, kiyosi::risk_measure::price)));
    CHECK(std::isfinite(risk_value(*result, kiyosi::risk_measure::delta)));
}

} // namespace
