#include <catch2/catch_test_macros.hpp>

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
    auto call = ito::make_european_option(ito::OptionType::Call, 100.0, expiry);
    REQUIRE(call.has_value());
    REQUIRE(call->type() == ito::OptionType::Call);
    REQUIRE(call->strike() == 100.0);
    REQUIRE(call->expiry() == expiry);

    const auto copy = *call;
    REQUIRE(copy == *call);
    REQUIRE(ito::make_european_option(ito::OptionType::Put, 100.0, expiry)->type() == ito::OptionType::Put);
}

TEST_CASE("European option factories reject invalid terms")
{
    const auto expiry = day(2030, 1, 1);
    REQUIRE(ito::make_european_option(ito::OptionType::Call, 0.0, expiry).error().category ==
            ito::error_category::invalid_option);
    REQUIRE(ito::make_european_option(ito::OptionType::Call, -1.0, expiry).error().message.find("strike") !=
            std::string::npos);
    REQUIRE_FALSE(ito::make_european_option(ito::OptionType::Call,
                                            std::numeric_limits<double>::infinity(), expiry).has_value());
    REQUIRE_FALSE(ito::make_european_option(static_cast<ito::OptionType>(99), 100.0, expiry).has_value());
    REQUIRE(ito::make_european_option(ito::OptionType::Call, 100.0, expiry, expiry).has_value());
    REQUIRE_FALSE(ito::make_european_option(ito::OptionType::Call, 100.0, day(2031, 1, 1), expiry).has_value());
}

TEST_CASE("BSM parameters and asset prices reject non-finite or non-positive values")
{
    auto valid = ito::make_bsm_parameters(0.05, 0.02, 0.2);
    REQUIRE(valid.has_value());
    REQUIRE(valid->risk_free_rate() == 0.05);
    REQUIRE(valid->dividend_yield() == 0.02);
    REQUIRE(valid->volatility() == 0.2);

    REQUIRE(ito::make_bsm_parameters(std::numeric_limits<double>::quiet_NaN(), 0.0, 0.2).error().category ==
            ito::error_category::invalid_parameter);
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
    auto context = ito::make_pricing_context(*parameters, 100.0, day(2025, 1, 1));
    REQUIRE(context.has_value());
    REQUIRE(context->asset_price().value() == 100.0);
    REQUIRE(context->valuation_date() == day(2025, 1, 1));
    const auto option = *ito::make_european_call(100.0, day(2030, 1, 1));
    REQUIRE(ito::make_pricing_context(*parameters, 100.0, day(2025, 1, 1), option).has_value());
    REQUIRE_FALSE(ito::make_pricing_context(*parameters, 100.0, day(2031, 1, 1), option).has_value());

    const ito::PricingResult result{1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0};
    REQUIRE(result.price() == result.value);
    const auto copy = result;
    REQUIRE(copy.rho == 11.0);
    STATIC_REQUIRE(std::is_copy_constructible_v<ito::PricingResult>);
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

    auto parameters = ito::make_bsm_parameters(0.05, 0.02, 0.2);
    auto context = ito::make_pricing_context(*parameters, 100.0, valuation, copied_calendar);
    REQUIRE(context.has_value());
    const auto context_copy = *context;
    REQUIRE(context_copy.calendar().is_trading_day(day(2025, 1, 2)));
}
