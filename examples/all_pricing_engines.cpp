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
    const auto price = result->get(kiyosi::risk_measure::price);
    if (!price) {
        std::cerr << instrument << " / " << engine << ": price unavailable\n";
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

    const auto european = *kiyosi::make_european_call(100.0, effective, expiry);
    const auto american = *kiyosi::make_american_put(100.0, effective, expiry);
    const auto cash_digital = *kiyosi::make_cash_or_nothing_option(
        kiyosi::option_type::call, 100.0, 10.0, effective, expiry);
    const auto asset_digital = *kiyosi::make_asset_or_nothing_option(
        kiyosi::option_type::call, 100.0, effective, expiry);
    const auto barrier = *kiyosi::make_barrier_option(
        kiyosi::option_type::call, 100.0, effective, expiry, 80.0, kiyosi::barrier_type::down_and_out);
    const auto binary_barrier = *kiyosi::make_cash_or_nothing_barrier_option(
        kiyosi::option_type::call, 100.0, effective, expiry, 80.0,
        kiyosi::barrier_type::down_and_out, 10.0);
    const auto geometric_asian = *kiyosi::make_geometric_average_option(
        kiyosi::option_type::call, 100.0, effective, effective, expiry);
    const auto arithmetic_asian = *kiyosi::make_arithmetic_average_option(
        kiyosi::option_type::call, 100.0, effective, effective, expiry);

    const auto accumulator = *kiyosi::make_accumulator(100.0, 110.0, 1.0, 2.0, 0.0, effective, expiry);
    const auto phoenix = *kiyosi::make_phoenix_option(
        0.08, 100.0, 80.0, {110.0}, {90.0}, 100.0, 60.0, {expiry},
        kiyosi::observation_frequency::daily, kiyosi::barrier_touch_status::none,
        1.0, effective, expiry);
    const auto snowball = *kiyosi::make_snowball_option(
        {0.08}, 0.05, 100.0, 80.0, {110.0}, 100.0, 60.0, {expiry},
        kiyosi::observation_frequency::daily, kiyosi::barrier_touch_status::none,
        1.0, effective, expiry);
    const auto binary_snowball = *kiyosi::make_binary_snowball_option(
        {0.08}, 0.05, 100.0, {110.0}, 100.0, 60.0, {expiry},
        kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    const auto ternary_snowball = *kiyosi::make_ternary_snowball_option(
        {0.08}, 0.05, 0.02, 100.0, 80.0, {110.0}, 100.0, 60.0, {expiry},
        kiyosi::observation_frequency::daily, kiyosi::barrier_touch_status::none,
        1.0, effective, expiry);

    bool ok = true;
    const auto price = [&](std::string_view instrument, std::string_view engine, const auto& value) {
        ok = print_price(instrument, engine, value) && ok;
    };

    price("EuropeanOption", "AnalyticEuropeanEngine", kiyosi::AnalyticEuropeanEngine{}.price(european, context));
    price("EuropeanOption", "BinomialEuropeanEngine", kiyosi::BinomialEuropeanEngine{128}.price(european, context));
    price("EuropeanOption", "CrrEngine", kiyosi::CrrEngine{128}.price(european, context));
    price("EuropeanOption", "FiniteDifferenceEuropeanEngine", kiyosi::FiniteDifferenceEuropeanEngine{80, 80}.price(european, context));
    price("EuropeanOption", "IntegralEuropeanEngine", kiyosi::IntegralEuropeanEngine{}.price(european, context));
    price("EuropeanOption", "MonteCarloEuropeanEngine", kiyosi::MonteCarloEuropeanEngine{5'000, 10, 42}.price(european, context));

    price("AmericanOption", "BinomialAmericanEngine", kiyosi::BinomialAmericanEngine{128}.price(american, context));
    price("AmericanOption", "CrrEngine", kiyosi::CrrEngine{128}.price(american, context));
    price("AmericanOption", "BjerksundStenslandAmericanEngine", kiyosi::BjerksundStenslandAmericanEngine{}.price(american, context));
    price("AmericanOption", "FiniteDifferenceAmericanEngine", kiyosi::FiniteDifferenceAmericanEngine{80, 80}.price(american, context));
    price("AmericanOption", "MonteCarloAmericanEngine", kiyosi::MonteCarloAmericanEngine{5'000, 20, 42}.price(american, context));

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
