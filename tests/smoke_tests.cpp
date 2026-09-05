#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>

#include <array>
#include <chrono>
#include <limits>
#include <string>
#include <type_traits>
#include <vector>

#include <ito/ito.hpp>

TEST_CASE("ito exposes its version")
{
    REQUIRE(ito::version_major == 0);
    REQUIRE(ito::version_minor == 1);
    REQUIRE(ito::version_patch == 0);
}

namespace {

ito::date day(int year, unsigned month, unsigned day_number)
{
    return ito::date{std::chrono::year{year} / std::chrono::month{month} / std::chrono::day{day_number}};
}

}

TEST_CASE("European options are validated immutable values")
{
    const auto expiry = day(2030, 1, 1);
    auto call = ito::make_european_option(ito::option_type::call, 100.0, expiry);
    REQUIRE(call.has_value());
    REQUIRE(call->type() == ito::option_type::call);
    REQUIRE(call->strike() == 100.0);
    REQUIRE(call->expiry() == expiry);

    const auto copy = *call;
    REQUIRE(copy == *call);
    REQUIRE(ito::make_european_option(ito::option_type::put, 100.0, expiry)->type() == ito::option_type::put);
}

TEST_CASE("European option factories reject invalid terms")
{
    const auto expiry = day(2030, 1, 1);
    REQUIRE(ito::make_european_option(ito::option_type::call, 0.0, expiry).error().category ==
            ito::error_category::invalid_strike);
    REQUIRE(ito::make_european_option(ito::option_type::call, -1.0, expiry).error().message.find("strike") !=
            std::string::npos);
    REQUIRE_FALSE(ito::make_european_option(ito::option_type::call,
                                            std::numeric_limits<double>::infinity(), expiry).has_value());
    REQUIRE_FALSE(ito::make_european_option(static_cast<ito::option_type>(99), 100.0, expiry).has_value());
    REQUIRE(ito::make_european_option(ito::option_type::call, 100.0, expiry, expiry).has_value());
    REQUIRE_FALSE(ito::make_european_option(ito::option_type::call, 100.0, day(2031, 1, 1), expiry).has_value());
}

TEST_CASE("BSM parameters and asset prices reject non-finite or non-positive values")
{
    auto valid = ito::make_bsm_parameters(0.05, 0.02, 0.2);
    REQUIRE(valid.has_value());
    REQUIRE(valid->risk_free_rate() == 0.05);
    REQUIRE(valid->dividend_yield() == 0.02);
    REQUIRE(valid->volatility() == 0.2);

    REQUIRE(ito::make_bsm_parameters(std::numeric_limits<double>::quiet_NaN(), 0.0, 0.2).error().category ==
            ito::error_category::invalid_rate);
    REQUIRE_FALSE(ito::make_bsm_parameters(0.0, std::numeric_limits<double>::infinity(), 0.2).has_value());
    REQUIRE_FALSE(ito::make_bsm_parameters(0.0, 0.0, 0.0).has_value());
    REQUIRE_FALSE(ito::make_bsm_parameters(0.0, 0.0, std::numeric_limits<double>::infinity()).has_value());

    REQUIRE(ito::make_asset_price(100.0).has_value());
    REQUIRE_FALSE(ito::make_asset_price(0.0).has_value());
    REQUIRE_FALSE(ito::make_asset_price(-1.0).has_value());
    REQUIRE_FALSE(ito::make_asset_price(std::numeric_limits<double>::quiet_NaN()).has_value());
}

TEST_CASE("Pricing context and result preserve their values")
{
    auto parameters = ito::make_bsm_parameters(0.05, 0.02, 0.2);
    auto context = ito::make_pricing_context(*parameters, *ito::make_asset_price(100.0), day(2025, 1, 1));
    REQUIRE(context.has_value());
    REQUIRE(context->asset_price().value() == 100.0);
    REQUIRE(context->valuation_date() == day(2025, 1, 1));
    REQUIRE(ito::make_pricing_context(*parameters, *ito::make_asset_price(100.0), day(2025, 1, 1)).has_value());

    const ito::PricingResult result{1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0};
    const auto copy = result;
    REQUIRE(copy.rho == 11.0);
    STATIC_REQUIRE(std::is_copy_constructible_v<ito::PricingResult>);
    STATIC_REQUIRE(std::is_copy_assignable_v<ito::PricingResult>);
    STATIC_REQUIRE(std::is_trivially_copyable_v<ito::Error>);
    STATIC_REQUIRE_FALSE(std::is_convertible_v<double, ito::AssetPrice>);
}

TEST_CASE("Dates, calendars, and observation schedules are value-safe")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry = valuation + std::chrono::days{10};
    REQUIRE(expiry > valuation);
    REQUIRE(ito::all_days_calendar().is_trading_day(expiry));
    REQUIRE_FALSE(ito::exchange_calendar().is_trading_day(day(2025, 1, 4)));

    auto custom = ito::make_trading_calendar(
        [](ito::date value) { return value == day(2025, 1, 2) || value == day(2025, 1, 3); }, 2);
    REQUIRE(custom.has_value());
    const auto copied_calendar = *custom;
    REQUIRE(copied_calendar.is_trading_day(day(2025, 1, 2)));
    REQUIRE(copied_calendar.annual_trading_days() == 2);

    const std::vector<ito::date> observations{day(2025, 1, 2), day(2025, 1, 3)};
    REQUIRE(ito::validate_schedule(observations, valuation, expiry, copied_calendar).has_value());
    REQUIRE_FALSE(ito::validate_schedule(
        std::vector<ito::date>{day(2025, 1, 4)}, valuation, expiry, copied_calendar).has_value());
    REQUIRE_FALSE(ito::validate_schedule(
        std::vector<ito::date>{day(2025, 1, 2), day(2025, 1, 2)}, valuation, expiry, copied_calendar).has_value());

    auto parameters = ito::make_bsm_parameters(0.05, 0.02, 0.2);
    auto context = ito::make_pricing_context(*parameters, *ito::make_asset_price(100.0), valuation, copied_calendar);
    REQUIRE(context.has_value());
    const auto context_copy = *context;
    REQUIRE(context_copy.calendar().is_trading_day(day(2025, 1, 2)));
}

TEST_CASE("Analytic European engine returns reviewed call value and Greeks")
{
    using Catch::Matchers::WithinAbs;

    const auto valuation = day(2025, 1, 6);
    const auto option = *ito::make_european_call(100.0, valuation + std::chrono::days{365});
    const auto parameters = *ito::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *ito::make_pricing_context(parameters, *ito::make_asset_price(100.0), valuation);

    const auto result = ito::AnalyticEuropeanEngine{}.price(option, context);
    REQUIRE(result.has_value());
    CHECK_THAT(result->value, WithinAbs(13.151137, 1e-6));
    CHECK_THAT(result->delta, WithinAbs(0.592749, 1e-6));
    CHECK_THAT(result->gamma, WithinAbs(0.012761, 1e-6));
    CHECK_THAT(result->speed, WithinAbs(-0.000234, 1e-6));
    CHECK_THAT(result->theta, WithinAbs(-0.019163, 1e-6));
    CHECK_THAT(result->charm, WithinAbs(-0.000115, 1e-6));
    CHECK_THAT(result->color, WithinAbs(0.000019, 1e-6));
    CHECK_THAT(result->vega, WithinAbs(0.382821, 1e-6));
    CHECK_THAT(result->vanna, WithinAbs(0.000638, 1e-6));
    CHECK_THAT(result->zomma, WithinAbs(-0.000431, 1e-6));
    CHECK_THAT(result->rho, WithinAbs(0.461238, 1e-6));
}

TEST_CASE("Analytic European calls and puts obey BSM identities")
{
    using Catch::Matchers::WithinAbs;

    const auto valuation = day(2025, 1, 6);
    const auto expiry = valuation + std::chrono::days{365};
    const auto call = *ito::make_european_call(100.0, expiry);
    const auto put = *ito::make_european_put(100.0, expiry);
    const auto parameters = *ito::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *ito::make_pricing_context(parameters, *ito::make_asset_price(100.0), valuation);
    const ito::AnalyticEuropeanEngine engine;

    const auto call_result = *engine.price(call, context);
    const auto put_result = *engine.price(put, context);
    CHECK_THAT(put_result.value, WithinAbs(10.225098, 1e-6));
    CHECK_THAT(call_result.value - put_result.value,
               WithinAbs(100.0 * std::exp(-0.01) - 100.0 * std::exp(-0.04), 1e-12));
    CHECK_THAT(call_result.delta - put_result.delta, WithinAbs(std::exp(-0.01), 1e-12));
    CHECK_THAT(call_result.gamma, WithinAbs(put_result.gamma, 1e-12));
    CHECK_THAT(call_result.speed, WithinAbs(put_result.speed, 1e-12));
    CHECK_THAT(call_result.color, WithinAbs(put_result.color, 1e-12));
    CHECK_THAT(call_result.vega, WithinAbs(put_result.vega, 1e-12));
    CHECK_THAT(call_result.vanna, WithinAbs(put_result.vanna, 1e-12));
    CHECK_THAT(call_result.zomma, WithinAbs(put_result.zomma, 1e-12));
    CHECK_THAT(put_result.theta, WithinAbs(-0.011346, 1e-6));
    CHECK_THAT(put_result.charm, WithinAbs(-0.000142, 1e-6));
    CHECK_THAT(put_result.rho, WithinAbs(-0.499552, 1e-6));
}

TEST_CASE("Analytic European engine remains finite one day before expiry")
{
    const auto valuation = day(2025, 1, 6);
    const auto option = *ito::make_european_call(100.0, valuation + std::chrono::days{1});
    const auto parameters = *ito::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *ito::make_pricing_context(parameters, *ito::make_asset_price(100.0), valuation);

    const auto result = ito::AnalyticEuropeanEngine{}.price(option, context);
    REQUIRE(result.has_value());
    for (const double value : std::array{result->value, result->delta, result->gamma,
                                        result->speed, result->theta, result->charm,
                                        result->color, result->vega, result->vanna,
                                        result->zomma, result->rho}) {
        CHECK(std::isfinite(value));
    }
}

TEST_CASE("Analytic European engine returns intrinsic value and zero Greeks at expiry")
{
    const auto expiry = day(2026, 1, 6);
    const auto call = *ito::make_european_call(100.0, expiry);
    const auto put = *ito::make_european_put(100.0, expiry);
    const auto parameters = *ito::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto call_context = *ito::make_pricing_context(parameters, *ito::make_asset_price(110.0), expiry);
    const auto put_context = *ito::make_pricing_context(parameters, *ito::make_asset_price(90.0), expiry);
    const ito::AnalyticEuropeanEngine engine;

    const auto call_result = *engine.price(call, call_context);
    const auto put_result = *engine.price(put, put_context);
    REQUIRE(call_result.value == 10.0);
    REQUIRE(put_result.value == 10.0);
    CHECK(call_result.delta == 0.0);
    CHECK(call_result.gamma == 0.0);
    CHECK(call_result.speed == 0.0);
    CHECK(call_result.theta == 0.0);
    CHECK(call_result.charm == 0.0);
    CHECK(call_result.color == 0.0);
    CHECK(call_result.vega == 0.0);
    CHECK(call_result.vanna == 0.0);
    CHECK(call_result.zomma == 0.0);
    CHECK(call_result.rho == 0.0);
}

TEST_CASE("Analytic European engine reports pricing and implied-volatility failures")
{
    using Catch::Matchers::WithinAbs;

    const auto valuation = day(2025, 1, 6);
    const auto expiry = valuation + std::chrono::days{365};
    const auto option = *ito::make_european_call(100.0, expiry);
    const auto parameters = *ito::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *ito::make_pricing_context(parameters, *ito::make_asset_price(100.0), valuation);
    const ito::AnalyticEuropeanEngine engine;

    const auto implied = engine.implied_volatility(option, context, 13.151137);
    REQUIRE(implied.has_value());
    CHECK_THAT(*implied, WithinAbs(0.3, 1e-7));

    const auto invalid_bracket = engine.implied_volatility(
        option, context, 13.151137, ito::ImpliedVolatilitySettings{1.0, 0.1});
    REQUIRE_FALSE(invalid_bracket.has_value());
    CHECK(invalid_bracket.error().category == ito::error_category::invalid_parameter);

    const auto unbracketed = engine.implied_volatility(option, context, 101.0);
    REQUIRE_FALSE(unbracketed.has_value());
    CHECK(unbracketed.error().category == ito::error_category::invalid_result);

    const auto non_finite_price = engine.implied_volatility(
        option, context, std::numeric_limits<double>::quiet_NaN());
    REQUIRE_FALSE(non_finite_price.has_value());
    CHECK(non_finite_price.error().category == ito::error_category::invalid_parameter);

    const auto unconverged = engine.implied_volatility(
        option, context, 13.151137,
        ito::ImpliedVolatilitySettings{.tolerance = 1e-15, .max_iterations = 1});
    REQUIRE_FALSE(unconverged.has_value());
    CHECK(unconverged.error().category == ito::error_category::invalid_result);

    const auto expired_context = *ito::make_pricing_context(parameters, *ito::make_asset_price(100.0), expiry);
    REQUIRE_FALSE(engine.implied_volatility(option, expired_context, 0.0).has_value());

    const auto stale_context = *ito::make_pricing_context(parameters, *ito::make_asset_price(100.0), expiry + std::chrono::days{1});
    const auto invalid_expiry = engine.price(option, stale_context);
    REQUIRE_FALSE(invalid_expiry.has_value());
    CHECK(invalid_expiry.error().category == ito::error_category::invalid_expiry);

    const auto extreme_parameters = *ito::make_bsm_parameters(-1000.0, 0.0, 0.3);
    const auto extreme_context = *ito::make_pricing_context(extreme_parameters, *ito::make_asset_price(100.0), valuation);
    const auto long_option = *ito::make_european_put(
        100.0, valuation + std::chrono::days{36500});
    const auto non_finite_result = engine.price(long_option, extreme_context);
    REQUIRE_FALSE(non_finite_result.has_value());
    CHECK(non_finite_result.error().category == ito::error_category::invalid_result);
}

TEST_CASE("Analytic European engine remains finite at near-zero volatility")
{
    const auto valuation = day(2025, 1, 6);
    const auto option = *ito::make_european_call(100.0, valuation + std::chrono::days{1});
    const auto parameters = *ito::make_bsm_parameters(0.04, 0.01, 1e-12);
    const auto context = *ito::make_pricing_context(parameters, *ito::make_asset_price(100.0), valuation);

    const auto result = ito::AnalyticEuropeanEngine{}.price(option, context);
    REQUIRE(result.has_value());
    CHECK(std::isfinite(result->value));
    CHECK(std::isfinite(result->delta));
    CHECK(std::isfinite(result->gamma));
}

TEST_CASE("Digital and barrier contracts validate and share pricing results")
{
    using Catch::Matchers::WithinAbs;
    const auto valuation = day(2025, 1, 6);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *ito::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *ito::make_pricing_context(parameters, *ito::make_asset_price(100.0), valuation);
    const auto cash_call = *ito::make_cash_or_nothing_option(ito::option_type::call, 100.0, 10.0, expiry);
    const auto cash_put = *ito::make_cash_or_nothing_option(ito::option_type::put, 100.0, 10.0, expiry);
    const ito::AnalyticDigitalEngine digital;
    const auto call_value = digital.price(cash_call, context);
    const auto put_value = digital.price(cash_put, context);
    REQUIRE(call_value.has_value());
    REQUIRE(put_value.has_value());
    CHECK_THAT(call_value->value + put_value->value, WithinAbs(10.0 * std::exp(-0.04), 1e-10));
    CHECK_FALSE(ito::make_cash_or_nothing_option(ito::option_type::call, 100.0, 0.0, expiry).has_value());

    const auto down_out = *ito::make_barrier_option(
        ito::option_type::call, 100.0, expiry, 90.0, ito::barrier_type::down_and_out);
    const auto down_in = *ito::make_barrier_option(
        ito::option_type::call, 100.0, expiry, 90.0, ito::barrier_type::down_and_in);
    const auto barrier_out = ito::AnalyticBarrierEngine{}.price(down_out, context);
    const auto barrier_in = ito::AnalyticBarrierEngine{}.price(down_in, context);
    const auto vanilla = ito::AnalyticEuropeanEngine{}.price(*ito::make_european_call(100.0, expiry), context);
    REQUIRE(barrier_out.has_value());
    REQUIRE(barrier_in.has_value());
    REQUIRE(vanilla.has_value());
    CHECK_THAT(barrier_out->value + barrier_in->value, WithinAbs(vanilla->value, 1e-5));

    const auto scheduled = ito::make_barrier_option(
        ito::option_type::call, 100.0, expiry, 90.0, ito::barrier_type::down_and_out,
        0.0, ito::rebate_timing::at_expiry, ito::observation_mode::scheduled,
        std::vector<ito::date>{valuation + std::chrono::days{30}});
    REQUIRE(scheduled.has_value());
    CHECK(ito::AnalyticBarrierEngine{}.price(*scheduled, context).has_value());
}

TEST_CASE("Already-hit barrier rebates respect expiry payment timing")
{
    const auto valuation = day(2025, 1, 6);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *ito::make_bsm_parameters(0.05, 0.0, 0.2);
    const auto context = *ito::make_pricing_context(parameters, *ito::make_asset_price(100.0), valuation);
    const auto barrier = *ito::make_barrier_option(
        ito::option_type::call, 100.0, expiry, 90.0, ito::barrier_type::up_and_out,
        10.0, ito::rebate_timing::at_expiry);

    const auto result = ito::AnalyticBarrierEngine{}.price(barrier, context);
    REQUIRE(result.has_value());
    CHECK_THAT(result->value, Catch::Matchers::WithinAbs(10.0 * std::exp(-0.05), 1e-12));
}

TEST_CASE("Binomial American engine prices expiry and validates steps")
{
    const auto expiry = day(2025, 1, 1);
    const auto option = *ito::make_european_put(100.0, expiry);
    const auto parameters = *ito::make_bsm_parameters(0.04, 0.0, 0.2);
    const auto context = *ito::make_pricing_context(parameters, *ito::make_asset_price(90.0), expiry);
    const ito::BinomialAmericanEngine engine;

    const auto priced = engine.price(option, context);
    REQUIRE(priced.has_value());
    CHECK(priced->value == 10.0);

    const auto invalid = engine.price(option, context, ito::BinomialAmericanSettings{0});
    REQUIRE_FALSE(invalid.has_value());
    CHECK(invalid.error().category == ito::error_category::invalid_parameter);
    const auto too_many = engine.price(option, context, ito::BinomialAmericanSettings{1'000'001});
    REQUIRE_FALSE(too_many.has_value());
    CHECK(too_many.error().category == ito::error_category::invalid_parameter);
}

TEST_CASE("Binomial American engine exercises puts and converges to European calls")
{
    using Catch::Matchers::WithinAbs;
    const auto valuation = day(2025, 1, 1);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *ito::make_bsm_parameters(0.05, 0.0, 0.2);
    const auto context = *ito::make_pricing_context(parameters, *ito::make_asset_price(90.0), valuation);
    const auto put = *ito::make_european_put(100.0, expiry);
    const auto call = *ito::make_european_call(100.0, expiry);
    const ito::BinomialAmericanEngine engine{ito::BinomialAmericanSettings{400}};

    const auto american_put = engine.price(put, context);
    const auto european_put = ito::AnalyticEuropeanEngine{}.price(put, context);
    REQUIRE(american_put.has_value());
    REQUIRE(european_put.has_value());
    CHECK(american_put->value > european_put->value);

    const auto at_the_money_context = *ito::make_pricing_context(parameters, *ito::make_asset_price(100.0), valuation);
    const auto american_call = engine.price(call, at_the_money_context);
    const auto european_call = ito::AnalyticEuropeanEngine{}.price(call, at_the_money_context);
    REQUIRE(american_call.has_value());
    REQUIRE(european_call.has_value());
    CHECK_THAT(american_call->value, WithinAbs(european_call->value, 0.02));
    CHECK(std::isfinite(american_call->delta));
    CHECK(std::isfinite(american_call->gamma));
}

TEST_CASE("Binomial American call and put values are symmetric at zero carry")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *ito::make_bsm_parameters(0.0, 0.0, 0.2);
    const auto context = *ito::make_pricing_context(parameters, *ito::make_asset_price(100.0), valuation);
    const auto call = *ito::make_european_call(100.0, expiry);
    const auto put = *ito::make_european_put(100.0, expiry);
    const ito::BinomialAmericanEngine engine{ito::BinomialAmericanSettings{200}};

    const auto call_result = engine.price(call, context);
    const auto put_result = engine.price(put, context);
    REQUIRE(call_result.has_value());
    REQUIRE(put_result.has_value());
    CHECK(std::abs(call_result->value - put_result->value) <= 1e-12);
}

TEST_CASE("Finite-difference engines validate grids and track reference engines")
{
    using Catch::Matchers::WithinAbs;
    const auto valuation = day(2025, 1, 1);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *ito::make_bsm_parameters(0.05, 0.0, 0.2);
    const auto context = *ito::make_pricing_context(parameters, *ito::make_asset_price(100.0), valuation);
    const auto call = *ito::make_european_call(100.0, expiry);
    const auto analytic = *ito::AnalyticEuropeanEngine{}.price(call, context);
    const ito::FiniteDifferenceSettings settings{200, 400, ito::finite_difference_scheme::crank_nicolson};
    const auto european = ito::FiniteDifferenceEuropeanEngine{settings}.price(call, context);
    REQUIRE(european.has_value());
    CHECK_THAT(european->value, WithinAbs(analytic.value, 0.05));
    for (const auto scheme : {ito::finite_difference_scheme::explicit_euler,
                              ito::finite_difference_scheme::implicit_euler}) {
        const auto result = ito::FiniteDifferenceEuropeanEngine{{200, 400, scheme}}.price(call, context);
        REQUIRE(result.has_value());
        CHECK_THAT(result->value, WithinAbs(analytic.value, 0.15));
    }
    const auto american = ito::FiniteDifferenceAmericanEngine{settings}.price(call, context);
    REQUIRE(american.has_value());
    CHECK_THAT(american->value, WithinAbs(analytic.value, 0.05));
    const auto put = *ito::make_european_put(100.0, expiry);
    const auto finite_put = ito::FiniteDifferenceAmericanEngine{settings}.price(put, context);
    const auto tree_put = ito::BinomialAmericanEngine{ito::BinomialAmericanSettings{400}}.price(put, context);
    REQUIRE(finite_put.has_value());
    REQUIRE(tree_put.has_value());
    CHECK_THAT(finite_put->value, WithinAbs(tree_put->value, 0.1));
    CHECK_FALSE(ito::FiniteDifferenceEuropeanEngine{{2, 10, ito::finite_difference_scheme::implicit_euler}}
                    .price(call, context).has_value());
    const auto expiry_context = *ito::make_pricing_context(parameters, *ito::make_asset_price(90.0), expiry);
    const auto expiry_put = *ito::make_european_put(100.0, expiry);
    const auto at_expiry = ito::FiniteDifferenceAmericanEngine{}.price(expiry_put, expiry_context);
    REQUIRE(at_expiry.has_value());
    CHECK(at_expiry->value == 10.0);
}
