#include <chrono>
#include <iostream>
#include <string_view>
#include <kiyosi/kiyosi.hpp>

namespace {

bool print_price(std::string_view instrument, std::string_view engine,
                 const kiyosi::Result<double>& result)
{
    if (!result) {
        std::cerr << instrument << " / " << engine << ": " << result.error().message << '\n';
        return false;
    }
    std::cout << instrument << " / " << engine << ": " << *result << '\n';
    return true;
}

} // namespace

int main()
{
    const kiyosi::Date effective_date{std::chrono::year{2025} / 1 / 1};
    const kiyosi::Date expiry_date{std::chrono::year{2026} / 1 / 1};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective_date);

    const auto european = *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, effective_date, expiry_date);
    const auto american = *kiyosi::make_american_option(kiyosi::OptionType::put, 100.0, effective_date, expiry_date);
    const auto cash_digital = *kiyosi::make_cash_or_nothing_option(
        kiyosi::OptionType::call, 100.0, 10.0, effective_date, expiry_date);
    const auto asset_digital = *kiyosi::make_asset_or_nothing_option(
        kiyosi::OptionType::call, 100.0, effective_date, expiry_date);
    const auto barrier = *kiyosi::make_barrier_option({.option_type = kiyosi::OptionType::call,
                                                       .strike = 100.0,
                                                       .effective_date = effective_date,
                                                       .expiry_date = expiry_date,
                                                       .barrier_level = 80.0,
                                                       .barrier_type = kiyosi::BarrierType::down_and_out});
    const auto binary_barrier = *kiyosi::make_cash_binary_barrier_option(
        {.option_type = kiyosi::OptionType::call,
         .strike = 100.0,
         .effective_date = effective_date,
         .expiry_date = expiry_date,
         .barrier_level = 80.0,
         .barrier_type = kiyosi::BarrierType::down_and_out},
        10.0);
    const auto averaging_start_date = effective_date;
    const auto geometric_asian = *kiyosi::make_geometric_average_option(
        kiyosi::OptionType::call, 100.0, averaging_start_date, effective_date, expiry_date);
    const auto arithmetic_asian = *kiyosi::make_arithmetic_average_option(
        kiyosi::OptionType::call, 100.0, averaging_start_date, effective_date, expiry_date);

    const auto accumulator = *kiyosi::make_accumulator({.strike = 100.0,
                                                        .knock_out_level = 110.0,
                                                        .daily_quantity = 1.0,
                                                        .acceleration_factor = 2.0,
                                                        .accumulated_quantity = 0.0,
                                                        .effective_date = effective_date,
                                                        .expiry_date = expiry_date});
    const auto phoenix = *kiyosi::make_phoenix_option({.coupon_rate = 0.08,
                                                       .initial_spot = 100.0,
                                                       .knock_in_level = 80.0,
                                                       .knock_out_levels = {110.0},
                                                       .coupon_barrier_levels = {90.0},
                                                       .upper_strike = 100.0,
                                                       .lower_strike = 60.0,
                                                       .observation_dates = {expiry_date},
                                                       .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day,
                                                       .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                       .principal_ratio = 1.0,
                                                       .effective_date = effective_date,
                                                       .expiry_date = expiry_date});
    const auto snowball = *kiyosi::make_both_down_snowball({.initial_coupon_rate = 0.08,
                                                            .coupon_rate_decrement = 0.01,
                                                            .initial_spot = 100.0,
                                                            .knock_in_level = 80.0,
                                                            .initial_knock_out_level = 110.0,
                                                            .knock_out_level_decrement = 5.0,
                                                            .observation_dates = {expiry_date},
                                                            .effective_date = effective_date,
                                                            .expiry_date = expiry_date});
    const auto binary_snowball = *kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {0.08},
                                                                       .maturity_coupon_rate = 0.05,
                                                                       .initial_spot = 100.0,
                                                                       .knock_out_levels = {110.0},
                                                                       .upper_strike = 100.0,
                                                                       .lower_strike = 60.0,
                                                                       .observation_dates = {expiry_date},
                                                                       .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                                       .principal_ratio = 1.0,
                                                                       .effective_date = effective_date,
                                                                       .expiry_date = expiry_date});
    const auto ternary_snowball = *kiyosi::make_ternary_snowball_option({.knock_out_coupon_rates = {0.08},
                                                                         .maturity_coupon_rate = 0.05,
                                                                         .minimum_coupon_rate = 0.02,
                                                                         .initial_spot = 100.0,
                                                                         .knock_in_level = 80.0,
                                                                         .knock_out_levels = {110.0},
                                                                         .upper_strike = 100.0,
                                                                         .lower_strike = 60.0,
                                                                         .observation_dates = {expiry_date},
                                                                         .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day,
                                                                         .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                                         .principal_ratio = 1.0,
                                                                         .effective_date = effective_date,
                                                                         .expiry_date = expiry_date});

    bool ok = true;
    const auto price = [&](std::string_view instrument, std::string_view engine, const auto& value) {
        ok = print_price(instrument, engine, value) && ok;
    };

    price("EuropeanOption", "AnalyticVanillaEngine", kiyosi::AnalyticVanillaEngine{}.price(european, context));
    price("EuropeanOption", "CoxRossRubinsteinVanillaEngine", kiyosi::CoxRossRubinsteinVanillaEngine{128}.price(european, context));
    price("EuropeanOption", "FiniteDifferenceVanillaEngine", kiyosi::FiniteDifferenceVanillaEngine{80, 80}.price(european, context));
    price("EuropeanOption", "QuadratureVanillaEngine", kiyosi::QuadratureVanillaEngine{}.price(european, context));
    price("EuropeanOption", "MonteCarloVanillaEngine", kiyosi::MonteCarloVanillaEngine{5'000, 10, 42}.price(european, context));

    price("AmericanOption", "CoxRossRubinsteinVanillaEngine", kiyosi::CoxRossRubinsteinVanillaEngine{128}.price(american, context));
    price("AmericanOption", "BjerksundStenslandVanillaEngine", kiyosi::BjerksundStenslandVanillaEngine{}.price(american, context));
    price("AmericanOption", "FiniteDifferenceVanillaEngine", kiyosi::FiniteDifferenceVanillaEngine{80, 80}.price(american, context));
    price("AmericanOption", "MonteCarloVanillaEngine", kiyosi::MonteCarloVanillaEngine{5'000, 20, 42}.price(american, context));

    price("CashOrNothingOption", "AnalyticDigitalEngine", kiyosi::AnalyticDigitalEngine{}.price(cash_digital, context));
    price("CashOrNothingOption", "FiniteDifferenceDigitalEngine", kiyosi::FiniteDifferenceDigitalEngine{80, 80}.price(cash_digital, context));
    price("CashOrNothingOption", "QuadratureDigitalEngine", kiyosi::QuadratureDigitalEngine{}.price(cash_digital, context));
    price("AssetOrNothingOption", "AnalyticDigitalEngine", kiyosi::AnalyticDigitalEngine{}.price(asset_digital, context));
    price("AssetOrNothingOption", "FiniteDifferenceDigitalEngine", kiyosi::FiniteDifferenceDigitalEngine{80, 80}.price(asset_digital, context));
    price("AssetOrNothingOption", "QuadratureDigitalEngine", kiyosi::QuadratureDigitalEngine{}.price(asset_digital, context));

    price("BarrierOption", "AnalyticBarrierEngine", kiyosi::AnalyticBarrierEngine{}.price(barrier, context));
    price("BarrierOption", "FiniteDifferenceBarrierEngine", kiyosi::FiniteDifferenceBarrierEngine{80, 80}.price(barrier, context));
    price("BinaryBarrierOption", "AnalyticBinaryBarrierEngine", kiyosi::AnalyticBinaryBarrierEngine{}.price(binary_barrier, context));
    price("GeometricAveragePriceOption", "AnalyticGeometricAveragePriceEngine", kiyosi::AnalyticGeometricAveragePriceEngine{}.price(geometric_asian, context));
    price("ArithmeticAveragePriceOption", "TurnbullWakemanArithmeticAveragePriceEngine", kiyosi::TurnbullWakemanArithmeticAveragePriceEngine{}.price(arithmetic_asian, context));

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

    for (const auto level : {kiyosi::GreeksLevel::basic, kiyosi::GreeksLevel::full}) {
        const auto result = kiyosi::AnalyticVanillaEngine{}.price_with_greeks(european, context, level);
        if (!result) {
            std::cerr << result.error().message << '\n';
            ok = false;
            continue;
        }
        std::cout << "EuropeanOption / analytic Greeks: delta="
                  << *result->require(kiyosi::RiskMeasure::delta)
                  << " gamma=" << *result->require(kiyosi::RiskMeasure::gamma) << '\n';
    }

    return ok ? 0 : 1;
}
