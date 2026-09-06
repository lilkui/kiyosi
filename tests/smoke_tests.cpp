#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>

#include <array>
#include <chrono>
#include <limits>
#include <string>
#include <type_traits>
#include <vector>

#include <kiyosi/kiyosi.hpp>

TEST_CASE("kiyosi exposes its version")
{
    REQUIRE(kiyosi::version_major == 0);
    REQUIRE(kiyosi::version_minor == 1);
    REQUIRE(kiyosi::version_patch == 0);
}

namespace {

kiyosi::date day(int year, unsigned month, unsigned day_number)
{
    return kiyosi::date{std::chrono::year{year} / std::chrono::month{month} / std::chrono::day{day_number}};
}

} // namespace

TEST_CASE("European options are validated immutable values")
{
    const auto expiry = day(2030, 1, 1);
    auto call = kiyosi::make_european_option(kiyosi::option_type::call, 100.0, expiry);
    REQUIRE(call.has_value());
    REQUIRE(call->type() == kiyosi::option_type::call);
    REQUIRE(call->strike() == 100.0);
    REQUIRE(call->expiry() == expiry);

    const auto copy = *call;
    REQUIRE(copy == *call);
    REQUIRE(kiyosi::make_european_option(kiyosi::option_type::put, 100.0, expiry)->type() == kiyosi::option_type::put);
}

TEST_CASE("European option factories reject invalid terms")
{
    const auto expiry = day(2030, 1, 1);
    REQUIRE(kiyosi::make_european_option(kiyosi::option_type::call, 0.0, expiry).error().category ==
            kiyosi::error_category::invalid_strike);
    REQUIRE(kiyosi::make_european_option(kiyosi::option_type::call, -1.0, expiry).error().message.find("strike") !=
            std::string::npos);
    REQUIRE_FALSE(kiyosi::make_european_option(kiyosi::option_type::call,
                                            std::numeric_limits<double>::infinity(), expiry)
                      .has_value());
    REQUIRE_FALSE(kiyosi::make_european_option(static_cast<kiyosi::option_type>(99), 100.0, expiry).has_value());
    REQUIRE(kiyosi::make_european_option(kiyosi::option_type::call, 100.0, expiry, expiry).has_value());
    REQUIRE_FALSE(kiyosi::make_european_option(kiyosi::option_type::call, 100.0, day(2031, 1, 1), expiry).has_value());
}

TEST_CASE("Exercise style and requested risk measures are explicit")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(100.0), valuation);
    const auto european = *kiyosi::make_european_call(100.0, expiry);
    const auto american = *kiyosi::make_american_call(100.0, expiry);
    REQUIRE(kiyosi::BinomialAmericanEngine{}.price(american, context).has_value());

    STATIC_REQUIRE((kiyosi::AnalyticEuropeanEngine::supported_risk_measures &
                    kiyosi::risk_bit(kiyosi::risk_measure::vega)) != 0);
    STATIC_REQUIRE((kiyosi::BinomialAmericanEngine::supported_risk_measures &
                    kiyosi::risk_bit(kiyosi::risk_measure::gamma)) != 0);

    const auto price_only = kiyosi::AnalyticEuropeanEngine{}.price(
        european, context, kiyosi::PricingRequest::price_only());
    REQUIRE(price_only.has_value());
    REQUIRE(price_only->has(kiyosi::risk_measure::price));
    REQUIRE_FALSE(price_only->has(kiyosi::risk_measure::delta));
    REQUIRE_FALSE(price_only->get(kiyosi::risk_measure::delta).has_value());
    CHECK(std::isnan(price_only->delta));

    const auto unsupported = kiyosi::BinomialAmericanEngine{}.price(
        american, context, kiyosi::PricingRequest{kiyosi::risk_bit(kiyosi::risk_measure::vega)});
    REQUIRE_FALSE(unsupported.has_value());
    CHECK(unsupported.error().category == kiyosi::error_category::unsupported_risk_measure);

    const auto incompatible = kiyosi::BinomialAmericanEngine{}.price(
        european, context, kiyosi::PricingRequest::price_only());
    REQUIRE_FALSE(incompatible.has_value());
    CHECK(incompatible.error().category == kiyosi::error_category::incompatible_exercise);
}

TEST_CASE("BSM parameters and asset prices reject non-finite or non-positive values")
{
    auto valid = kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    REQUIRE(valid.has_value());
    REQUIRE(valid->risk_free_rate() == 0.05);
    REQUIRE(valid->dividend_yield() == 0.02);
    REQUIRE(valid->volatility() == 0.2);

    REQUIRE(kiyosi::make_bsm_parameters(std::numeric_limits<double>::quiet_NaN(), 0.0, 0.2).error().category ==
            kiyosi::error_category::invalid_rate);
    REQUIRE_FALSE(kiyosi::make_bsm_parameters(0.0, std::numeric_limits<double>::infinity(), 0.2).has_value());
    REQUIRE_FALSE(kiyosi::make_bsm_parameters(0.0, 0.0, 0.0).has_value());
    REQUIRE_FALSE(kiyosi::make_bsm_parameters(0.0, 0.0, std::numeric_limits<double>::infinity()).has_value());

    REQUIRE(kiyosi::make_asset_price(100.0).has_value());
    REQUIRE_FALSE(kiyosi::make_asset_price(0.0).has_value());
    REQUIRE_FALSE(kiyosi::make_asset_price(-1.0).has_value());
    REQUIRE_FALSE(kiyosi::make_asset_price(std::numeric_limits<double>::quiet_NaN()).has_value());
}

TEST_CASE("Pricing context and result preserve their values")
{
    auto parameters = kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    auto context = kiyosi::make_pricing_context(*parameters, *kiyosi::make_asset_price(100.0), day(2025, 1, 1));
    REQUIRE(context.has_value());
    REQUIRE(context->asset_price().value() == 100.0);
    REQUIRE(context->valuation_date() == day(2025, 1, 1));
    REQUIRE(kiyosi::make_pricing_context(*parameters, *kiyosi::make_asset_price(100.0), day(2025, 1, 1)).has_value());

    const kiyosi::PricingResult result{1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0};
    const auto copy = result;
    REQUIRE(copy.rho == 11.0);
    STATIC_REQUIRE(std::is_copy_constructible_v<kiyosi::PricingResult>);
    STATIC_REQUIRE(std::is_copy_assignable_v<kiyosi::PricingResult>);
    STATIC_REQUIRE(std::is_trivially_copyable_v<kiyosi::Error>);
    STATIC_REQUIRE_FALSE(std::is_convertible_v<double, kiyosi::AssetPrice>);
}

TEST_CASE("Dates, calendars, and observation schedules are value-safe")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry = valuation + std::chrono::days{10};
    REQUIRE(expiry > valuation);
    REQUIRE(kiyosi::all_days_calendar().is_trading_day(expiry));
    REQUIRE_FALSE(kiyosi::exchange_calendar().is_trading_day(day(2025, 1, 4)));

    auto custom = kiyosi::make_trading_calendar(
        [](kiyosi::date value) { return value == day(2025, 1, 2) || value == day(2025, 1, 3); }, 2);
    REQUIRE(custom.has_value());
    const auto copied_calendar = *custom;
    REQUIRE(copied_calendar.is_trading_day(day(2025, 1, 2)));
    REQUIRE(copied_calendar.annual_trading_days() == 2);

    const std::vector<kiyosi::date> observations{day(2025, 1, 2), day(2025, 1, 3)};
    REQUIRE(kiyosi::validate_schedule(observations, valuation, expiry, copied_calendar).has_value());
    REQUIRE_FALSE(kiyosi::validate_schedule(
                      std::vector<kiyosi::date>{day(2025, 1, 4)}, valuation, expiry, copied_calendar)
                      .has_value());
    REQUIRE_FALSE(kiyosi::validate_schedule(
                      std::vector<kiyosi::date>{day(2025, 1, 2), day(2025, 1, 2)}, valuation, expiry, copied_calendar)
                      .has_value());

    auto parameters = kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    auto context = kiyosi::make_pricing_context(*parameters, *kiyosi::make_asset_price(100.0), valuation, copied_calendar);
    REQUIRE(context.has_value());
    const auto context_copy = *context;
    REQUIRE(context_copy.calendar().is_trading_day(day(2025, 1, 2)));
}

TEST_CASE("Analytic European engine returns reviewed call value and Greeks")
{
    using Catch::Matchers::WithinAbs;

    const auto valuation = day(2025, 1, 6);
    const auto option = *kiyosi::make_european_call(100.0, valuation + std::chrono::days{365});
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(100.0), valuation);

    const auto result = kiyosi::AnalyticEuropeanEngine{}.price(option, context);
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
    const auto call = *kiyosi::make_european_call(100.0, expiry);
    const auto put = *kiyosi::make_european_put(100.0, expiry);
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(100.0), valuation);
    const kiyosi::AnalyticEuropeanEngine engine;

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
    const auto option = *kiyosi::make_european_call(100.0, valuation + std::chrono::days{1});
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(100.0), valuation);

    const auto result = kiyosi::AnalyticEuropeanEngine{}.price(option, context);
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
    const auto call = *kiyosi::make_european_call(100.0, expiry);
    const auto put = *kiyosi::make_european_put(100.0, expiry);
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto call_context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(110.0), expiry);
    const auto put_context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(90.0), expiry);
    const kiyosi::AnalyticEuropeanEngine engine;

    const auto call_result = *engine.price(call, call_context);
    const auto put_result = *engine.price(put, put_context);
    REQUIRE(call_result.value == 10.0);
    REQUIRE(put_result.value == 10.0);
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

TEST_CASE("Time and schedules share explicit day-count and calendar rules")
{
    const auto start = day(2025, 1, 1);
    const auto end = day(2025, 1, 6);
    const auto fraction = kiyosi::year_fraction(start, end);
    REQUIRE(fraction.has_value());
    CHECK_THAT(*fraction, Catch::Matchers::WithinAbs(5.0 / 365.0, 1e-15));

    const auto noon = kiyosi::start_of_day(start) + std::chrono::hours{12};
    const auto context = kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.01, 0.0, 0.2), *kiyosi::make_asset_price(100.0), noon);
    REQUIRE(context.has_value());
    CHECK(context->valuation_date() == start);
    CHECK(context->valuation_time().time_since_epoch() == noon.time_since_epoch());
    CHECK(kiyosi::exchange_calendar().trading_days_between(start, end) == 3);
    CHECK_THAT(kiyosi::exchange_calendar().trading_year_fraction(start, end),
               Catch::Matchers::WithinAbs(3.0 / 252.0, 1e-15));

    const auto schedule = kiyosi::make_observation_schedule(
        std::vector<kiyosi::date>{day(2025, 1, 2), day(2025, 1, 3)}, start, end,
        kiyosi::exchange_calendar());
    REQUIRE(schedule.has_value());
    const auto barrier = kiyosi::make_barrier_option(
        kiyosi::option_type::call, 100.0, end, 90.0, kiyosi::barrier_type::down_and_out,
        0.0, kiyosi::rebate_timing::at_expiry, kiyosi::observation_mode::scheduled, *schedule);
    REQUIRE(barrier.has_value());
    CHECK(barrier->schedule() == *schedule);
}

TEST_CASE("Analytic European engine remains finite at near-zero volatility")
{
    const auto valuation = day(2025, 1, 6);
    const auto option = *kiyosi::make_european_call(100.0, valuation + std::chrono::days{1});
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 1e-12);
    const auto context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(100.0), valuation);

    const auto result = kiyosi::AnalyticEuropeanEngine{}.price(option, context);
    REQUIRE(result.has_value());
    CHECK(std::isfinite(result->value));
    CHECK(std::isfinite(result->delta));
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
    const auto call_price = *engine.price(call, call_context, kiyosi::PricingRequest::price_only());
    const auto call_implied = engine.implied_volatility(call, call_context, call_price.value);
    REQUIRE(call_implied.has_value());
    CHECK_THAT(*call_implied, WithinAbs(0.35, 1e-7));

    const auto deep_in_the_money = *kiyosi::make_european_call(20.0, expiry);
    const auto deep_itm_price = *engine.price(deep_in_the_money, call_context,
                                              kiyosi::PricingRequest::price_only());
    const auto deep_itm_implied = engine.implied_volatility(deep_in_the_money, call_context,
                                                             deep_itm_price.value);
    REQUIRE(deep_itm_implied.has_value());
    CHECK_THAT(*deep_itm_implied, WithinAbs(0.35, 1e-6));

    const auto deep_out_of_the_money = *kiyosi::make_european_call(180.0, expiry);
    const auto deep_otm_price = *engine.price(deep_out_of_the_money, call_context,
                                              kiyosi::PricingRequest::price_only());
    const auto deep_otm_implied = engine.implied_volatility(deep_out_of_the_money, call_context,
                                                             deep_otm_price.value);
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
    for (const double value : std::array{result->value, result->delta, result->gamma,
                                         result->speed, result->theta, result->charm,
                                         result->color, result->vega, result->vanna,
                                         result->zomma, result->rho}) {
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
    CHECK(std::isfinite(result->value));
    CHECK(std::isfinite(result->delta));
}

TEST_CASE("Digital and barrier contracts validate and share pricing results")
{
    using Catch::Matchers::WithinAbs;
    const auto valuation = day(2025, 1, 6);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(100.0), valuation);
    const auto cash_call = *kiyosi::make_cash_or_nothing_option(kiyosi::option_type::call, 100.0, 10.0, expiry);
    const auto cash_put = *kiyosi::make_cash_or_nothing_option(kiyosi::option_type::put, 100.0, 10.0, expiry);
    const kiyosi::AnalyticDigitalEngine digital;
    const auto call_value = digital.price(cash_call, context);
    const auto put_value = digital.price(cash_put, context);
    REQUIRE(call_value.has_value());
    REQUIRE(put_value.has_value());
    const auto all_requested = digital.price(cash_call, context, {});
    REQUIRE(all_requested.has_value());
    CHECK(all_requested->has(kiyosi::risk_measure::price));
    CHECK(all_requested->has(kiyosi::risk_measure::delta));
    CHECK(all_requested->has(kiyosi::risk_measure::gamma));
    CHECK_FALSE(all_requested->has(kiyosi::risk_measure::vega));
    CHECK_THAT(call_value->value + put_value->value, WithinAbs(10.0 * std::exp(-0.04), 1e-10));
    CHECK_FALSE(kiyosi::make_cash_or_nothing_option(kiyosi::option_type::call, 100.0, 0.0, expiry).has_value());

    const auto down_out = *kiyosi::make_barrier_option(
        kiyosi::option_type::call, 100.0, expiry, 90.0, kiyosi::barrier_type::down_and_out);
    const auto down_in = *kiyosi::make_barrier_option(
        kiyosi::option_type::call, 100.0, expiry, 90.0, kiyosi::barrier_type::down_and_in);
    const auto barrier_out = kiyosi::AnalyticBarrierEngine{}.price(down_out, context);
    const auto barrier_in = kiyosi::AnalyticBarrierEngine{}.price(down_in, context);
    const auto vanilla = kiyosi::AnalyticEuropeanEngine{}.price(*kiyosi::make_european_call(100.0, expiry), context);
    REQUIRE(barrier_out.has_value());
    REQUIRE(barrier_in.has_value());
    REQUIRE(vanilla.has_value());
    CHECK_THAT(barrier_out->value + barrier_in->value, WithinAbs(vanilla->value, 1e-5));

    const auto scheduled = kiyosi::make_barrier_option(
        kiyosi::option_type::call, 100.0, expiry, 90.0, kiyosi::barrier_type::down_and_out,
        0.0, kiyosi::rebate_timing::at_expiry, kiyosi::observation_mode::scheduled,
        std::vector<kiyosi::date>{valuation + std::chrono::days{30}});
    REQUIRE(scheduled.has_value());
    CHECK(kiyosi::AnalyticBarrierEngine{}.price(*scheduled, context).has_value());
}

TEST_CASE("Already-hit barrier rebates respect expiry payment timing")
{
    const auto valuation = day(2025, 1, 6);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(100.0), valuation);
    const auto barrier = *kiyosi::make_barrier_option(
        kiyosi::option_type::call, 100.0, expiry, 90.0, kiyosi::barrier_type::up_and_out,
        10.0, kiyosi::rebate_timing::at_expiry);

    const auto result = kiyosi::AnalyticBarrierEngine{}.price(barrier, context);
    REQUIRE(result.has_value());
    CHECK_THAT(result->value, Catch::Matchers::WithinAbs(10.0 * std::exp(-0.05), 1e-12));
}

TEST_CASE("Barrier hit rebates use the finite first-hit payment decomposition")
{
    const auto valuation = day(2025, 1, 6);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.03, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(100.0), valuation);
    const auto barrier = *kiyosi::make_barrier_option(
        kiyosi::option_type::call, 1'000'000'000.0, expiry, 110.0, kiyosi::barrier_type::up_and_out,
        10.0, kiyosi::rebate_timing::at_hit);

    const auto result = kiyosi::AnalyticBarrierEngine{}.price(barrier, context);
    REQUIRE(result.has_value());
    const double distance = std::log(1.1);
    const double root = std::sqrt(2.0 * 0.05 * 0.2 * 0.2);
    const auto normal_cdf = [](double value) { return 0.5 * std::erfc(-value / std::sqrt(2.0)); };
    const double discounted_hit = std::exp(-root * distance / (0.2 * 0.2)) *
                                      normal_cdf((root - distance) / 0.2) +
                                  std::exp(root * distance / (0.2 * 0.2)) *
                                      normal_cdf((-root - distance) / 0.2);
    CHECK_THAT(result->value, Catch::Matchers::WithinAbs(10.0 * discounted_hit, 1e-6));
}

TEST_CASE("Barrier hit rebates reject an unstable negative-rate limit")
{
    const auto valuation = day(2025, 1, 6);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(-0.02, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(100.0), valuation);
    const auto barrier = *kiyosi::make_barrier_option(
        kiyosi::option_type::call, 1'000'000'000.0, expiry, 110.0, kiyosi::barrier_type::up_and_out,
        10.0, kiyosi::rebate_timing::at_hit);

    const auto result = kiyosi::AnalyticBarrierEngine{}.price(barrier, context);
    REQUIRE_FALSE(result.has_value());
    CHECK(result.error().category == kiyosi::error_category::invalid_result);
}

TEST_CASE("Binomial American engine prices expiry and validates steps")
{
    const auto expiry = day(2025, 1, 1);
    const auto option = *kiyosi::make_american_put(100.0, expiry);
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(90.0), expiry);
    const kiyosi::BinomialAmericanEngine engine;

    const auto priced = engine.price(option, context);
    REQUIRE(priced.has_value());
    CHECK(priced->value == 10.0);

    const auto invalid = engine.price(option, context, kiyosi::BinomialAmericanSettings{0});
    REQUIRE_FALSE(invalid.has_value());
    CHECK(invalid.error().category == kiyosi::error_category::invalid_parameter);
    const auto too_many = engine.price(option, context, kiyosi::BinomialAmericanSettings{1'000'001});
    REQUIRE_FALSE(too_many.has_value());
    CHECK(too_many.error().category == kiyosi::error_category::invalid_parameter);
}

TEST_CASE("Binomial American engine does not expose gamma below two steps")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(100.0), valuation);
    const auto option = *kiyosi::make_american_call(100.0, expiry);

    const auto result = kiyosi::BinomialAmericanEngine{kiyosi::BinomialAmericanSettings{1}}.price(option, context);
    REQUIRE(result.has_value());
    CHECK(result->has(kiyosi::risk_measure::price));
    CHECK(result->has(kiyosi::risk_measure::delta));
    CHECK_FALSE(result->has(kiyosi::risk_measure::gamma));
    CHECK_FALSE(result->get(kiyosi::risk_measure::gamma).has_value());
    CHECK(std::isnan(result->gamma));
}

TEST_CASE("Binomial American engine exercises puts and converges to European calls")
{
    using Catch::Matchers::WithinAbs;
    const auto valuation = day(2025, 1, 1);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(90.0), valuation);
    const auto put = *kiyosi::make_american_put(100.0, expiry);
    const auto european_put_option = *kiyosi::make_european_put(100.0, expiry);
    const auto call = *kiyosi::make_european_call(100.0, expiry);
    const auto american_call_option = *kiyosi::make_american_call(100.0, expiry);
    const kiyosi::BinomialAmericanEngine engine{kiyosi::BinomialAmericanSettings{400}};

    const auto american_put = engine.price(put, context);
    const auto european_put = kiyosi::AnalyticEuropeanEngine{}.price(european_put_option, context);
    REQUIRE(american_put.has_value());
    REQUIRE(european_put.has_value());
    CHECK(american_put->value > european_put->value);

    const auto at_the_money_context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(100.0), valuation);
    const auto american_call = engine.price(american_call_option, at_the_money_context);
    const auto european_call = kiyosi::AnalyticEuropeanEngine{}.price(call, at_the_money_context);
    REQUIRE(american_call.has_value());
    REQUIRE(european_call.has_value());
    CHECK_THAT(american_call->value, WithinAbs(european_call->value, 0.02));
    CHECK(std::isfinite(american_call->delta));
    CHECK(std::isfinite(american_call->gamma));

    const auto high_resolution = kiyosi::BinomialAmericanEngine{kiyosi::BinomialAmericanSettings{1200}}
                                     .price(american_call_option, at_the_money_context);
    REQUIRE(high_resolution.has_value());
    CHECK_THAT(high_resolution->value, WithinAbs(european_call->value, 0.01));
}

TEST_CASE("Binomial American call and put values are symmetric at zero carry")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.0, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(100.0), valuation);
    const auto call = *kiyosi::make_american_call(100.0, expiry);
    const auto put = *kiyosi::make_american_put(100.0, expiry);
    const kiyosi::BinomialAmericanEngine engine{kiyosi::BinomialAmericanSettings{200}};

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
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(100.0), valuation);
    const auto call = *kiyosi::make_european_call(100.0, expiry);
    const auto american_call = *kiyosi::make_american_call(100.0, expiry);
    const auto analytic = *kiyosi::AnalyticEuropeanEngine{}.price(call, context);
    const kiyosi::FiniteDifferenceSettings settings{200, 400, kiyosi::finite_difference_scheme::crank_nicolson};
    const auto european = kiyosi::FiniteDifferenceEuropeanEngine{settings}.price(call, context);
    REQUIRE(european.has_value());
    CHECK_THAT(european->value, WithinAbs(analytic.value, 0.05));
    for (const auto scheme : {kiyosi::finite_difference_scheme::explicit_euler,
                              kiyosi::finite_difference_scheme::implicit_euler}) {
        const auto result = kiyosi::FiniteDifferenceEuropeanEngine{{200, 400, scheme}}.price(call, context);
        REQUIRE(result.has_value());
        CHECK_THAT(result->value, WithinAbs(analytic.value, 0.15));
    }
    const auto american = kiyosi::FiniteDifferenceAmericanEngine{settings}.price(american_call, context);
    REQUIRE(american.has_value());
    CHECK_THAT(american->value, WithinAbs(analytic.value, 0.05));
    const auto put = *kiyosi::make_american_put(100.0, expiry);
    const auto finite_put = kiyosi::FiniteDifferenceAmericanEngine{settings}.price(put, context);
    const auto tree_put = kiyosi::BinomialAmericanEngine{kiyosi::BinomialAmericanSettings{400}}.price(put, context);
    REQUIRE(finite_put.has_value());
    REQUIRE(tree_put.has_value());
    CHECK_THAT(finite_put->value, WithinAbs(tree_put->value, 0.1));
    CHECK_FALSE(kiyosi::FiniteDifferenceEuropeanEngine{{2, 10, kiyosi::finite_difference_scheme::implicit_euler}}
                    .price(call, context)
                    .has_value());
    const auto expiry_context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(90.0), expiry);
    const auto expiry_put = *kiyosi::make_american_put(100.0, expiry);
    const auto at_expiry = kiyosi::FiniteDifferenceAmericanEngine{}.price(expiry_put, expiry_context);
    REQUIRE(at_expiry.has_value());
    CHECK(at_expiry->value == 10.0);
}
