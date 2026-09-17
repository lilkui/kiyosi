#include <chrono>
#include <iostream>
#include <string_view>
#include <kiyosi/kiyosi.hpp>

namespace {

bool print_price(std::string_view instrument, std::string_view engine,
                 const kiyosi::result<kiyosi::PricingResult>& result)
{
    if (!result) {
        std::cerr << instrument << " / " << engine << ": " << result.error().message << '\n';
        return false;
    }
    const auto price = result->require(kiyosi::risk_measure::price);
    if (!price) {
        std::cerr << instrument << " / " << engine << ": " << price.error().message << '\n';
        return false;
    }
    std::cout << instrument << " / " << engine << ": " << *price << '\n';
    return true;
}

} // namespace

int main()
{
    const kiyosi::date effective{std::chrono::year{2025} / 1 / 1};
    const kiyosi::date expiry{std::chrono::year{2026} / 1 / 1};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective);

    const auto european = *kiyosi::make_european_option(kiyosi::option_type::call, 100.0, effective, expiry);
    const auto american = *kiyosi::make_american_option(kiyosi::option_type::put, 100.0, effective, expiry);
    const auto cash_digital = *kiyosi::make_cash_or_nothing_option(
        kiyosi::option_type::call, 100.0, 10.0, effective, expiry);
    const auto asset_digital = *kiyosi::make_asset_or_nothing_option(
        kiyosi::option_type::call, 100.0, effective, expiry);
    const auto barrier = *kiyosi::make_barrier_option({.type = kiyosi::option_type::call,
                                                       .strike = 100.0,
                                                       .effective = effective,
                                                       .expiry = expiry,
                                                       .barrier = 80.0,
                                                       .barrier_kind = kiyosi::barrier_type::down_and_out});
    const auto binary_barrier = *kiyosi::make_cash_binary_barrier_option(
        {.type = kiyosi::option_type::call,
         .strike = 100.0,
         .effective = effective,
         .expiry = expiry,
         .barrier = 80.0,
         .barrier_kind = kiyosi::barrier_type::down_and_out},
        10.0);
    const auto geometric_asian = *kiyosi::make_geometric_average_option(
        kiyosi::option_type::call, 100.0, effective, effective, expiry);
    const auto arithmetic_asian = *kiyosi::make_arithmetic_average_option(
        kiyosi::option_type::call, 100.0, effective, effective, expiry);

    const auto accumulator = *kiyosi::make_accumulator({.strike = 100.0,
                                                        .knock_out = 110.0,
                                                        .daily_quantity = 1.0,
                                                        .acceleration = 2.0,
                                                        .accumulated_quantity = 0.0,
                                                        .effective = effective,
                                                        .expiry = expiry});
    const auto phoenix = *kiyosi::make_phoenix_option({.coupon_rate = 0.08,
                                                       .initial_price = 100.0,
                                                       .knock_in_price = 80.0,
                                                       .knock_out_prices = {110.0},
                                                       .coupon_barriers = {90.0},
                                                       .upper_strike = 100.0,
                                                       .lower_strike = 60.0,
                                                       .observation_dates = {expiry},
                                                       .frequency = kiyosi::observation_frequency::daily,
                                                       .touch_status = kiyosi::barrier_touch_status::none,
                                                       .principal_ratio = 1.0,
                                                       .effective = effective,
                                                       .expiry = expiry});
    const auto snowball = *kiyosi::make_snowball_option({.knock_out_coupon_rates = {0.08},
                                                         .maturity_coupon_rate = 0.05,
                                                         .initial_price = 100.0,
                                                         .knock_in_price = 80.0,
                                                         .knock_out_prices = {110.0},
                                                         .upper_strike = 100.0,
                                                         .lower_strike = 60.0,
                                                         .observation_dates = {expiry},
                                                         .frequency = kiyosi::observation_frequency::daily,
                                                         .touch_status = kiyosi::barrier_touch_status::none,
                                                         .principal_ratio = 1.0,
                                                         .effective = effective,
                                                         .expiry = expiry});
    const auto binary_snowball = *kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {0.08},
                                                                       .maturity_coupon_rate = 0.05,
                                                                       .initial_price = 100.0,
                                                                       .knock_out_prices = {110.0},
                                                                       .upper_strike = 100.0,
                                                                       .lower_strike = 60.0,
                                                                       .observation_dates = {expiry},
                                                                       .touch_status = kiyosi::barrier_touch_status::none,
                                                                       .principal_ratio = 1.0,
                                                                       .effective = effective,
                                                                       .expiry = expiry});
    const auto ternary_snowball = *kiyosi::make_ternary_snowball_option({.knock_out_coupon_rates = {0.08},
                                                                         .maturity_coupon_rate = 0.05,
                                                                         .minimal_coupon_rate = 0.02,
                                                                         .initial_price = 100.0,
                                                                         .knock_in_price = 80.0,
                                                                         .knock_out_prices = {110.0},
                                                                         .upper_strike = 100.0,
                                                                         .lower_strike = 60.0,
                                                                         .observation_dates = {expiry},
                                                                         .frequency = kiyosi::observation_frequency::daily,
                                                                         .touch_status = kiyosi::barrier_touch_status::none,
                                                                         .principal_ratio = 1.0,
                                                                         .effective = effective,
                                                                         .expiry = expiry});

    bool ok = true;
    const auto price = [&](std::string_view instrument, std::string_view engine, const auto& value) {
        ok = print_price(instrument, engine, value) && ok;
    };

    price("EuropeanOption", "AnalyticVanillaEngine", kiyosi::AnalyticVanillaEngine{}.price(european, context));
    price("EuropeanOption", "CrrVanillaEngine", kiyosi::CrrVanillaEngine{128}.price(european, context));
    price("EuropeanOption", "FiniteDifferenceVanillaEngine", kiyosi::FiniteDifferenceVanillaEngine{80, 80}.price(european, context));
    price("EuropeanOption", "IntegralVanillaEngine", kiyosi::IntegralVanillaEngine{}.price(european, context));
    price("EuropeanOption", "MonteCarloVanillaEngine", kiyosi::MonteCarloVanillaEngine{5'000, 10, 42}.price(european, context));

    price("AmericanOption", "CrrVanillaEngine", kiyosi::CrrVanillaEngine{128}.price(american, context));
    price("AmericanOption", "BjerksundStenslandVanillaEngine", kiyosi::BjerksundStenslandVanillaEngine{}.price(american, context));
    price("AmericanOption", "FiniteDifferenceVanillaEngine", kiyosi::FiniteDifferenceVanillaEngine{80, 80}.price(american, context));
    price("AmericanOption", "MonteCarloVanillaEngine", kiyosi::MonteCarloVanillaEngine{5'000, 20, 42}.price(american, context));

    price("EuropeanCashOrNothingOption", "AnalyticDigitalEngine", kiyosi::AnalyticDigitalEngine{}.price(cash_digital, context));
    price("EuropeanCashOrNothingOption", "FiniteDifferenceDigitalEngine", kiyosi::FiniteDifferenceDigitalEngine{80, 80}.price(cash_digital, context));
    price("EuropeanCashOrNothingOption", "IntegralDigitalEngine", kiyosi::IntegralDigitalEngine{}.price(cash_digital, context));
    price("EuropeanAssetOrNothingOption", "AnalyticDigitalEngine", kiyosi::AnalyticDigitalEngine{}.price(asset_digital, context));
    price("EuropeanAssetOrNothingOption", "FiniteDifferenceDigitalEngine", kiyosi::FiniteDifferenceDigitalEngine{80, 80}.price(asset_digital, context));
    price("EuropeanAssetOrNothingOption", "IntegralDigitalEngine", kiyosi::IntegralDigitalEngine{}.price(asset_digital, context));

    price("BarrierOption", "AnalyticBarrierEngine", kiyosi::AnalyticBarrierEngine{}.price(barrier, context));
    price("BarrierOption", "FiniteDifferenceBarrierEngine", kiyosi::FiniteDifferenceBarrierEngine{80, 80}.price(barrier, context));
    price("BinaryBarrierOption", "AnalyticBinaryBarrierEngine", kiyosi::AnalyticBinaryBarrierEngine{}.price(binary_barrier, context));
    price("GeometricAverageOption", "GeometricAverageAsianEngine", kiyosi::GeometricAverageAsianEngine{}.price(geometric_asian, context));
    price("ArithmeticAverageOption", "ArithmeticAverageAsianEngine", kiyosi::ArithmeticAverageAsianEngine{}.price(arithmetic_asian, context));

    price("Accumulator", "FiniteDifferenceAccumulatorEngine", kiyosi::FiniteDifferenceAccumulatorEngine{80, 80}.price(accumulator, context));
    price("Accumulator", "MonteCarloAccumulatorEngine", kiyosi::MonteCarloAccumulatorEngine{2'000, 42}.price(accumulator, context));
    price("PhoenixOption", "FiniteDifferencePhoenixEngine", kiyosi::FiniteDifferencePhoenixEngine{80, 80}.price(phoenix, context));
    price("PhoenixOption", "MonteCarloPhoenixEngine", kiyosi::MonteCarloPhoenixEngine{2'000, 42}.price(phoenix, context));
    price("SnowballOption", "FiniteDifferenceSnowballEngine", kiyosi::FiniteDifferenceSnowballEngine{80, 80}.price(snowball, context));
    price("SnowballOption", "MonteCarloSnowballEngine", kiyosi::MonteCarloSnowballEngine{2'000, 42}.price(snowball, context));
    price("BinarySnowballOption", "FiniteDifferenceBinarySnowballEngine", kiyosi::FiniteDifferenceBinarySnowballEngine{80, 80}.price(binary_snowball, context));
    price("BinarySnowballOption", "MonteCarloBinarySnowballEngine", kiyosi::MonteCarloBinarySnowballEngine{2'000, 42}.price(binary_snowball, context));
    price("TernarySnowballOption", "FiniteDifferenceTernarySnowballEngine", kiyosi::FiniteDifferenceTernarySnowballEngine{80, 80}.price(ternary_snowball, context));
    price("TernarySnowballOption", "MonteCarloTernarySnowballEngine", kiyosi::MonteCarloTernarySnowballEngine{2'000, 42}.price(ternary_snowball, context));

    return ok ? 0 : 1;
}
