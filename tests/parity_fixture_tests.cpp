#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_string.hpp>

#include <filesystem>
#include <fstream>
#include <cmath>
#include <chrono>
#include <ranges>
#include <set>
#include <sstream>
#include <string>

#include "parity_fixture.hpp"

namespace {

std::filesystem::path fixture_path()
{
    return std::filesystem::path{KIYOSI_SOURCE_DIR} / "tests" / "fixtures" / "european_bsm.tsv";
}

std::filesystem::path cpu_fixture_path()
{
    return std::filesystem::path{KIYOSI_SOURCE_DIR} / "tests" / "fixtures" / "cpu_parity.tsv";
}

std::string fixture_text()
{
    std::ifstream input(fixture_path());
    REQUIRE(input.good());
    return {std::istreambuf_iterator<char>{input}, std::istreambuf_iterator<char>{}};
}

kiyosi::PricingContext standard_context()
{
    const auto valuation = kiyosi::date{std::chrono::year{2025} / 1 / 6};
    return *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3), *kiyosi::make_asset_price(100.0), valuation);
}

kiyosi::date standard_expiry()
{
    return kiyosi::date{std::chrono::year{2026} / 1 / 6};
}

template <typename PriceResult>
void check_price(const kiyosi::test::ParityCase& fixture, const PriceResult& priced)
{
    INFO("case=" << fixture.case_id << " instrument=" << fixture.instrument << " engine=" << fixture.engine);
    REQUIRE(priced.has_value());
    REQUIRE(fixture.outputs.contains("price"));
    REQUIRE(fixture.tolerances.contains("price"));
    REQUIRE(priced->get(kiyosi::risk_measure::price).has_value());
    CAPTURE(*priced->get(kiyosi::risk_measure::price));
    CHECK(std::abs(*priced->get(kiyosi::risk_measure::price) - fixture.outputs.at("price")) <=
          fixture.tolerances.at("price"));
}

} // namespace

TEST_CASE("European parity fixtures compare value, Greeks, and implied volatility")
{
    const auto fixtures = kiyosi::test::load_parity_fixtures(fixture_path());
    REQUIRE(fixtures.size() == 2);
    const kiyosi::AnalyticEuropeanEngine engine;
    for (const auto& fixture : fixtures) {
        const auto option = kiyosi::make_european_option(fixture.option, fixture.strike, fixture.expiry);
        REQUIRE(option.has_value());
        const auto parameters = kiyosi::make_bsm_parameters(
            fixture.risk_free_rate, fixture.dividend_yield, fixture.volatility);
        REQUIRE(parameters.has_value());
        const auto context = kiyosi::make_pricing_context(
            *parameters, *kiyosi::make_asset_price(fixture.spot), fixture.valuation_date);
        REQUIRE(context.has_value());
        const auto priced = engine.price(*option, *context);
        REQUIRE(priced.has_value());
        const auto implied = engine.implied_volatility(*option, *context, fixture.observed_price);
        REQUIRE(implied.has_value());
        kiyosi::test::check_fixture(fixture, *priced, *implied);
    }
}

TEST_CASE("Parity fixture parser reports malformed rows")
{
    auto text = fixture_text();
    const auto option = text.find("\tcall\t");
    REQUIRE(option != std::string::npos);
    text.replace(option + 1, 4, "future");
    auto parse = [&] {
        std::istringstream input{text};
        return kiyosi::test::parse_parity_fixtures(input);
    };
    CHECK_THROWS_WITH(parse(),
                      Catch::Matchers::ContainsSubstring("fixture row 4"));

    text = fixture_text();
    const auto number = text.find("\t100\t100\t0.04");
    REQUIRE(number != std::string::npos);
    text.replace(number + 9, 4, "oops");
    CHECK_THROWS_WITH(parse(),
                      Catch::Matchers::ContainsSubstring("invalid"));

    text = fixture_text();
    const auto date = text.find("2025-01-06");
    REQUIRE(date != std::string::npos);
    text.replace(date + 5, 2, "0x");
    CHECK_THROWS_WITH(parse(), Catch::Matchers::ContainsSubstring("valuation_date"));

    text = fixture_text();
    const auto tolerance = text.find("\t0.00001\t");
    REQUIRE(tolerance != std::string::npos);
    text.replace(tolerance + 1, 7, "-1");
    CHECK_THROWS_WITH(parse(), Catch::Matchers::ContainsSubstring("non-negative"));
}

TEST_CASE("Parity fixture tolerances are inclusive and mismatch reports are useful")
{
    CHECK(kiyosi::test::within_tolerance(2.0, 1.0, 1.0));
    CHECK_FALSE(kiyosi::test::within_tolerance(2.000001, 1.0, 1.0));

    auto fixture = kiyosi::test::load_parity_fixtures(fixture_path()).front();
    fixture.value += 1.0;
    const kiyosi::PricingResult actual{{kiyosi::risk_measure::price, fixture.value - 1.0},
                                       {kiyosi::risk_measure::delta, fixture.delta},
                                       {kiyosi::risk_measure::gamma, fixture.gamma},
                                       {kiyosi::risk_measure::speed, fixture.speed},
                                       {kiyosi::risk_measure::theta, fixture.theta},
                                       {kiyosi::risk_measure::charm, fixture.charm},
                                       {kiyosi::risk_measure::color, fixture.color},
                                       {kiyosi::risk_measure::vega, fixture.vega},
                                       {kiyosi::risk_measure::vanna, fixture.vanna},
                                       {kiyosi::risk_measure::zomma, fixture.zomma},
                                       {kiyosi::risk_measure::rho, fixture.rho}};
    const auto failures = kiyosi::test::compare_fixture(fixture, actual, fixture.implied_volatility);
    REQUIRE_FALSE(failures.empty());
    const auto message = failures.front().message();
    CHECK(message.find("case='reviewed-call-1'") != std::string::npos);
    CHECK(message.find("output='value'") != std::string::npos);
    CHECK(message.find("expected=") != std::string::npos);
    CHECK(message.find("actual=") != std::string::npos);
    CHECK(message.find("tolerance=") != std::string::npos);
}

TEST_CASE("CPU parity manifest covers instruments, engines, and numerical metadata")
{
    const auto cases = kiyosi::test::load_parity_cases(cpu_fixture_path());
    REQUIRE(cases.size() >= 20);
    std::vector<std::string> names;
    names.reserve(cases.size());
    for (const auto& value : cases) {
        INFO("case=" << value.case_id << " instrument=" << value.instrument << " engine=" << value.engine);
        REQUIRE_FALSE(value.case_id.empty());
        REQUIRE_FALSE(value.instrument.empty());
        REQUIRE_FALSE(value.engine.empty());
        REQUIRE_FALSE(value.variant.empty());
        CHECK(value.provenance.source_revision == std::string{kiyosi::test::pinned_reference_revision});
        CHECK_FALSE(value.provenance.source_symbol.empty());
        CHECK_FALSE(value.provenance.convention.empty());
        CHECK((value.provenance.reference_kind == "analytic" ||
               value.provenance.reference_kind == "discretized" ||
               value.provenance.reference_kind == "statistical"));
        REQUIRE(value.outputs.size() == value.tolerances.size());
        for (const auto& [name, tolerance] : value.tolerances) {
            REQUIRE(value.outputs.contains(name));
            CHECK(tolerance >= 0.0);
        names.push_back(value.instrument + "/" + value.engine);
        }
    }
    CHECK(std::ranges::any_of(names, [](const auto& name) { return name == "Accumulator/MonteCarloAccumulatorEngine"; }));
    CHECK(std::ranges::any_of(names, [](const auto& name) { return name == "PhoenixOption/FiniteDifferencePhoenixEngine"; }));
    CHECK(std::ranges::any_of(names, [](const auto& name) { return name == "BarrierOption/FiniteDifferenceBarrierEngine"; }));
    CHECK(std::ranges::any_of(cases, [](const auto& value) { return value.validation.has_value(); }));
    CHECK(std::ranges::any_of(cases, [](const auto& value) { return value.convergence.has_value(); }));
    CHECK(std::ranges::any_of(cases, [](const auto& value) { return value.monte_carlo.has_value(); }));
}

TEST_CASE("CPU parity manifest closes every concrete engine and contract variant")
{
    const auto cases = kiyosi::test::load_parity_cases(cpu_fixture_path());
    const std::set<std::string> required_engines{
        "AnalyticBarrierEngine", "AnalyticBinaryBarrierEngine", "AnalyticDigitalEngine",
        "AnalyticEuropeanEngine", "ArithmeticAverageAsianEngine", "BinomialAmericanEngine",
        "BinomialEuropeanEngine", "BjerksundStenslandAmericanEngine", "CrrEngine",
        "FiniteDifferenceAccumulatorEngine", "FiniteDifferenceAmericanEngine",
        "FiniteDifferenceBarrierEngine", "FiniteDifferenceBinarySnowballEngine",
        "FiniteDifferenceDigitalEngine", "FiniteDifferenceEuropeanEngine",
        "FiniteDifferencePhoenixEngine", "FiniteDifferenceSnowballEngine",
        "FiniteDifferenceTernarySnowballEngine", "GeometricAverageAsianEngine",
        "IntegralDigitalEngine", "IntegralEuropeanEngine", "MonteCarloAccumulatorEngine",
        "MonteCarloAmericanEngine", "MonteCarloBinarySnowballEngine",
        "MonteCarloEuropeanEngine", "MonteCarloPhoenixEngine", "MonteCarloSnowballEngine",
        "MonteCarloTernarySnowballEngine"};
    const std::set<std::string> required_instruments{
        "Accumulator", "AmericanOption", "ArithmeticAverageOption", "BarrierOption",
        "BermudanOption", "BinaryBarrierOption", "BinarySnowballOption", "EuropeanAssetOrNothingOption",
        "EuropeanCashOrNothingOption", "EuropeanOption", "GeometricAverageOption",
        "PhoenixOption", "SnowballOption", "TernarySnowballOption"};
    const std::set<std::string> required_pairs{
        "Accumulator/FiniteDifferenceAccumulatorEngine", "Accumulator/MonteCarloAccumulatorEngine",
        "AmericanOption/BinomialAmericanEngine", "AmericanOption/FiniteDifferenceAmericanEngine",
        "AmericanOption/MonteCarloAmericanEngine", "AmericanOption/BjerksundStenslandAmericanEngine",
        "ArithmeticAverageOption/ArithmeticAverageAsianEngine", "BarrierOption/AnalyticBarrierEngine",
        "BarrierOption/FiniteDifferenceBarrierEngine", "BermudanOption/BinomialAmericanEngine",
        "BinaryBarrierOption/AnalyticBinaryBarrierEngine", "BinarySnowballOption/FiniteDifferenceBinarySnowballEngine",
        "BinarySnowballOption/MonteCarloBinarySnowballEngine", "EuropeanAssetOrNothingOption/AnalyticDigitalEngine",
        "EuropeanCashOrNothingOption/AnalyticDigitalEngine", "EuropeanCashOrNothingOption/FiniteDifferenceDigitalEngine",
        "EuropeanCashOrNothingOption/IntegralDigitalEngine", "EuropeanOption/AnalyticEuropeanEngine",
        "EuropeanOption/BinomialEuropeanEngine", "EuropeanOption/CrrEngine",
        "EuropeanOption/FiniteDifferenceEuropeanEngine", "EuropeanOption/IntegralEuropeanEngine",
        "EuropeanOption/MonteCarloEuropeanEngine", "GeometricAverageOption/GeometricAverageAsianEngine",
        "PhoenixOption/FiniteDifferencePhoenixEngine", "PhoenixOption/MonteCarloPhoenixEngine",
        "SnowballOption/FiniteDifferenceSnowballEngine", "SnowballOption/MonteCarloSnowballEngine",
        "TernarySnowballOption/FiniteDifferenceTernarySnowballEngine",
        "TernarySnowballOption/MonteCarloTernarySnowballEngine"};
    std::set<std::string> actual_engines;
    std::set<std::string> actual_instruments;
    std::set<std::string> actual_pairs;
    bool has_settlement = false;
    bool has_monitoring = false;
    bool has_calendar = false;
    for (const auto& value : cases) {
        actual_engines.insert(value.engine);
        actual_instruments.insert(value.instrument);
        actual_pairs.insert(value.instrument + "/" + value.engine);
        has_settlement |= value.inputs.contains("settlement");
        has_monitoring |= value.inputs.contains("monitoring");
        has_calendar |= value.inputs.contains("calendar");
        if (value.engine.find("MonteCarlo") != std::string::npos && !value.validation.has_value())
            REQUIRE(value.monte_carlo.has_value());
        if (value.engine.find("Binomial") != std::string::npos ||
            value.engine.find("FiniteDifference") != std::string::npos)
            if (!value.validation.has_value() && value.instrument != "BermudanOption")
            REQUIRE(value.convergence.has_value());
        if (value.validation.has_value())
            CHECK(value.outputs.empty());
        else
            REQUIRE(value.outputs.contains("price"));
    }
    CHECK(actual_engines == required_engines);
    CHECK(actual_instruments == required_instruments);
    CHECK(actual_pairs == required_pairs);
    CHECK(has_settlement);
    CHECK(has_monitoring);
    CHECK(has_calendar);
}

TEST_CASE("Reviewed analytic CPU fixture matches every exposed risk measure")
{
    const auto cases = kiyosi::test::load_parity_cases(cpu_fixture_path());
    const auto fixture = std::ranges::find_if(cases, [](const auto& value) {
        return value.case_id == "european-analytic";
    });
    REQUIRE(fixture != cases.end());
    const auto valuation = kiyosi::date{std::chrono::year{2025} / 1 / 6};
    const auto expiry = kiyosi::date{std::chrono::year{2026} / 1 / 6};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3), *kiyosi::make_asset_price(100.0), valuation);
    const auto option = *kiyosi::make_european_call(100.0, expiry);
    const auto result = *kiyosi::AnalyticEuropeanEngine{}.price(option, context);
    const std::map<std::string, kiyosi::risk_measure> measures{
        {"price", kiyosi::risk_measure::price}, {"delta", kiyosi::risk_measure::delta},
        {"gamma", kiyosi::risk_measure::gamma}, {"speed", kiyosi::risk_measure::speed},
        {"theta", kiyosi::risk_measure::theta}, {"charm", kiyosi::risk_measure::charm},
        {"color", kiyosi::risk_measure::color}, {"vega", kiyosi::risk_measure::vega},
        {"vanna", kiyosi::risk_measure::vanna}, {"zomma", kiyosi::risk_measure::zomma},
        {"rho", kiyosi::risk_measure::rho}};
    for (const auto& [name, measure] : measures) {
        INFO("case=" << fixture->case_id << " output=" << name);
        REQUIRE(fixture->outputs.contains(name));
        REQUIRE(fixture->tolerances.contains(name));
        REQUIRE(result.get(measure).has_value());
        CHECK(std::abs(*result.get(measure) - fixture->outputs.at(name)) <= fixture->tolerances.at(name));
    }
}

TEST_CASE("Vanilla and digital engines match reviewed CPU fixtures")
{
    const auto cases = kiyosi::test::load_parity_cases(cpu_fixture_path());
    const auto context = standard_context();
    const auto expiry = standard_expiry();
    const auto call = *kiyosi::make_european_call(100.0, context.valuation_date(), expiry);
    const auto put = *kiyosi::make_european_put(100.0, context.valuation_date(), expiry);
    const auto find = [&](std::string_view case_id) -> const kiyosi::test::ParityCase& {
        const auto found = std::ranges::find_if(cases, [&](const auto& value) { return value.case_id == case_id; });
        REQUIRE(found != cases.end());
        REQUIRE(found->provenance.source_revision == std::string{kiyosi::test::pinned_reference_revision});
        return *found;
    };

    check_price(find("european-analytic"), kiyosi::AnalyticEuropeanEngine{}.price(call, context));
    check_price(find("european-binomial"), kiyosi::BinomialEuropeanEngine{200}.price(call, context));
    check_price(find("crr-vanilla"), kiyosi::CrrEngine{200}.price(call, context));
    check_price(find("european-integral"), kiyosi::IntegralEuropeanEngine{}.price(put, context));
    check_price(find("european-fd"), kiyosi::FiniteDifferenceEuropeanEngine{200, 200}.price(call, context));
    const auto cash_call = *kiyosi::make_cash_or_nothing_option(kiyosi::option_type::call, 100.0, 10.0, expiry);
    const auto asset_put = *kiyosi::make_asset_or_nothing_option(kiyosi::option_type::put, 100.0, expiry);
    check_price(find("cash-digital-analytic"), kiyosi::AnalyticDigitalEngine{}.price(cash_call, context));
    check_price(find("asset-digital-analytic"), kiyosi::AnalyticDigitalEngine{}.price(asset_put, context));
    check_price(find("digital-integral"), kiyosi::IntegralDigitalEngine{}.price(cash_call, context));
    const auto digital_put = *kiyosi::make_cash_or_nothing_option(kiyosi::option_type::put, 100.0, 10.0, expiry);
    check_price(find("digital-fd"), kiyosi::FiniteDifferenceDigitalEngine{200, 200}.price(digital_put, context));
}

TEST_CASE("Asian engines match pinned terms and market assumptions")
{
    const auto effective = kiyosi::date{std::chrono::year{2025} / 1 / 6};
    const auto valuation = effective + std::chrono::days{90};
    const auto expiry = effective + std::chrono::days{180};
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(100.0), valuation);
    const auto cases = kiyosi::test::load_parity_cases(cpu_fixture_path());
    const auto find = [&](std::string_view id) -> const auto& {
        const auto found = std::ranges::find_if(cases, [&](const auto& value) { return value.case_id == id; });
        REQUIRE(found != cases.end());
        return *found;
    };
    const auto arithmetic_call = *kiyosi::make_arithmetic_average_option(
        kiyosi::option_type::call, 100.0, effective, effective, expiry, 101.0);
    const auto arithmetic_put = *kiyosi::make_arithmetic_average_option(
        kiyosi::option_type::put, 100.0, effective, effective, expiry, 101.0);
    check_price(find("asian-arithmetic-call"), kiyosi::ArithmeticAverageAsianEngine{}.price(arithmetic_call, context));
    check_price(find("asian-arithmetic-put"), kiyosi::ArithmeticAverageAsianEngine{}.price(arithmetic_put, context));
    const auto geometric = *kiyosi::make_geometric_average_option(
        kiyosi::option_type::put, 85.0, effective, effective, effective + std::chrono::days{91});
    const auto geometric_context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.05, -0.03, 0.2), *kiyosi::make_asset_price(80.0), effective);
    check_price(find("asian-geometric"), kiyosi::GeometricAverageAsianEngine{}.price(geometric, geometric_context));
    const auto sse_context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.05, -0.03, 0.2), *kiyosi::make_asset_price(80.0), effective,
        kiyosi::sse_calendar());
    check_price(find("asian-geometric-sse"), kiyosi::GeometricAverageAsianEngine{}.price(geometric, sse_context));
}

TEST_CASE("CPU parity public properties cover payoff, in-out, convergence, and seeded paths")
{
    const auto valuation = kiyosi::date{std::chrono::year{2025} / 1 / 6};
    const auto expiry = kiyosi::date{std::chrono::year{2026} / 1 / 6};
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(100.0), valuation);

    const auto call = *kiyosi::make_european_call(100.0, expiry);
    const auto put = *kiyosi::make_european_put(100.0, expiry);
    const auto analytic_call = *kiyosi::AnalyticEuropeanEngine{}.price(call, context);
    const auto analytic_put = *kiyosi::AnalyticEuropeanEngine{}.price(put, context);
    const auto call_price = *analytic_call.get(kiyosi::risk_measure::price);
    const auto put_price = *analytic_put.get(kiyosi::risk_measure::price);
    CHECK(std::abs(call_price - put_price -
                   (100.0 * std::exp(-0.01) - 100.0 * std::exp(-0.04))) < 1e-10);

    const auto in = *kiyosi::make_barrier_option(kiyosi::option_type::call, 100.0, expiry, 130.0,
                                                  kiyosi::barrier_type::up_and_in);
    const auto out = *kiyosi::make_barrier_option(kiyosi::option_type::call, 100.0, expiry, 130.0,
                                                   kiyosi::barrier_type::up_and_out);
    const auto barrier_in = *kiyosi::AnalyticBarrierEngine{}.price(in, context);
    const auto barrier_out = *kiyosi::AnalyticBarrierEngine{}.price(out, context);
    CHECK(std::abs(*barrier_in.get(kiyosi::risk_measure::price) +
                       *barrier_out.get(kiyosi::risk_measure::price) - call_price) < 2e-5);

    const auto coarse = *kiyosi::BinomialEuropeanEngine{32}.price(call, context);
    const auto fine = *kiyosi::BinomialEuropeanEngine{128}.price(call, context);
    CHECK(std::abs(*fine.get(kiyosi::risk_measure::price) - call_price) <
          std::abs(*coarse.get(kiyosi::risk_measure::price) - call_price));

    const auto cases = kiyosi::test::load_parity_cases(cpu_fixture_path());
    const auto binomial_fixture = std::ranges::find_if(cases, [](const auto& value) {
        return value.case_id == "european-binomial";
    });
    REQUIRE(binomial_fixture != cases.end());
    REQUIRE(binomial_fixture->convergence.has_value());
    for (const auto resolution : binomial_fixture->convergence->resolutions) {
        const auto result = kiyosi::BinomialEuropeanEngine{static_cast<int>(resolution)}.price(call, context);
        REQUIRE(result.has_value());
        CHECK(std::isfinite(*result->get(kiyosi::risk_measure::price)));
    }
    const auto final_binomial = *kiyosi::BinomialEuropeanEngine{
        static_cast<int>(binomial_fixture->convergence->resolutions.back())}.price(call, context);
    CHECK(std::abs(*final_binomial.get(kiyosi::risk_measure::price) -
                   binomial_fixture->convergence->reference) <= binomial_fixture->convergence->tolerance);

    const auto monte_carlo_fixture = std::ranges::find_if(cases, [](const auto& value) {
        return value.case_id == "european-mc";
    });
    REQUIRE(monte_carlo_fixture != cases.end());
    REQUIRE(monte_carlo_fixture->monte_carlo.has_value());
    const auto& monte_carlo_metadata = *monte_carlo_fixture->monte_carlo;
    const kiyosi::MonteCarloEuropeanEngine monte_carlo{
        static_cast<int>(monte_carlo_metadata.paths), static_cast<int>(monte_carlo_metadata.steps),
        monte_carlo_metadata.seed};
    const auto first = *monte_carlo.price(call, context);
    const auto second = *monte_carlo.price(call, context);
    CHECK(*first.get(kiyosi::risk_measure::price) == *second.get(kiyosi::risk_measure::price));
    CHECK(std::abs(*first.get(kiyosi::risk_measure::price) - monte_carlo_fixture->outputs.at("price")) <=
          monte_carlo_metadata.tolerance);
}

TEST_CASE("Typed structured finite-difference fixtures execute every concrete product")
{
    const auto effective = kiyosi::date{std::chrono::year{2025} / 1 / 1};
    const auto expiry = kiyosi::date{std::chrono::year{2026} / 1 / 1};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), *kiyosi::make_asset_price(100.0), effective);
    const std::vector<kiyosi::date> observations{effective + std::chrono::days{90},
                                                 effective + std::chrono::days{181},
                                                 effective + std::chrono::days{273}, expiry};
    const std::vector<double> knock_outs{110.0, 108.0, 106.0, 104.0};
    const std::vector<double> coupons{0.02, 0.04, 0.06, 0.08};
    const auto accumulator = *kiyosi::make_accumulator(100.0, 110.0, 1.0, 2.0, 3.0, effective, expiry);
    const auto phoenix = *kiyosi::make_phoenix_option(
        0.02, 100.0, 75.0, knock_outs, {90.0, 90.0, 90.0, 90.0}, 100.0, 60.0, observations,
        kiyosi::observation_frequency::daily, kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    const auto snowball = *kiyosi::make_snowball_option(
        coupons, 0.08, 100.0, 75.0, knock_outs, 100.0, 60.0, observations,
        kiyosi::observation_frequency::daily, kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    const auto binary = *kiyosi::make_binary_snowball_option(
        coupons, 0.08, 100.0, knock_outs, 100.0, 60.0, observations,
        kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    const auto ternary = *kiyosi::make_ternary_snowball_option(
        coupons, 0.08, 0.02, 100.0, 75.0, knock_outs, 100.0, 60.0, observations,
        kiyosi::observation_frequency::daily, kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    const auto cases = kiyosi::test::load_parity_cases(cpu_fixture_path());
    const auto fixture = [&](std::string_view case_id) -> const auto& {
        const auto found = std::ranges::find_if(cases, [&](const auto& value) { return value.case_id == case_id; });
        REQUIRE(found != cases.end());
        return *found;
    };
    check_price(fixture("accumulator-fd"), kiyosi::FiniteDifferenceAccumulatorEngine{{80, 1024}}.price(accumulator, context));
    check_price(fixture("structured-fd"), kiyosi::FiniteDifferencePhoenixEngine{{80, 1024}}.price(phoenix, context));
    check_price(fixture("snowball-fd"), kiyosi::FiniteDifferenceSnowballEngine{{80, 1024}}.price(snowball, context));
    check_price(fixture("binary-snowball-fd"), kiyosi::FiniteDifferenceBinarySnowballEngine{{80, 1024}}.price(binary, context));
    check_price(fixture("ternary-snowball-fd"), kiyosi::FiniteDifferenceTernarySnowballEngine{{80, 1024}}.price(ternary, context));
}

TEST_CASE("Seeded Monte Carlo fixtures execute every concrete Monte Carlo engine repeatably")
{
    const auto effective = kiyosi::date{std::chrono::year{2025} / 1 / 1};
    const auto expiry = kiyosi::date{std::chrono::year{2026} / 1 / 1};
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), *kiyosi::make_asset_price(100.0), effective);
    const auto call = *kiyosi::make_european_call(100.0, effective, expiry);
    const auto put = *kiyosi::make_american_put(100.0, effective, expiry);
    const std::vector<kiyosi::date> observations{effective + std::chrono::days{90},
                                                 effective + std::chrono::days{181},
                                                 effective + std::chrono::days{273}, expiry};
    const std::vector<double> knock_outs{110.0, 108.0, 106.0, 104.0};
    const std::vector<double> coupons{0.02, 0.04, 0.06, 0.08};
    const auto phoenix = *kiyosi::make_phoenix_option(
        0.02, 100.0, 75.0, knock_outs, {90.0, 90.0, 90.0, 90.0}, 100.0, 60.0, observations,
        kiyosi::observation_frequency::daily, kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    const auto snowball = *kiyosi::make_snowball_option(
        coupons, 0.08, 100.0, 75.0, knock_outs, 100.0, 60.0, observations,
        kiyosi::observation_frequency::daily, kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    const auto binary = *kiyosi::make_binary_snowball_option(
        coupons, 0.08, 100.0, knock_outs, 100.0, 60.0, observations,
        kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    const auto ternary = *kiyosi::make_ternary_snowball_option(
        coupons, 0.08, 0.02, 100.0, 75.0, knock_outs, 100.0, 60.0, observations,
        kiyosi::observation_frequency::daily, kiyosi::barrier_touch_status::none, 1.0, effective, expiry);
    const auto cases = kiyosi::test::load_parity_cases(cpu_fixture_path());
    const auto require_repeatable = [&](std::string_view case_id, const auto& instrument, const auto& first_engine) {
        const auto found = std::ranges::find_if(cases, [&](const auto& value) { return value.case_id == case_id; });
        REQUIRE(found != cases.end());
        REQUIRE(found->monte_carlo.has_value());
        const auto first = first_engine.price(instrument, context);
        const auto second = first_engine.price(instrument, context);
        REQUIRE(first.has_value());
        REQUIRE(second.has_value());
        REQUIRE(first->get(kiyosi::risk_measure::price).has_value());
        REQUIRE(second->get(kiyosi::risk_measure::price).has_value());
        CHECK(*first->get(kiyosi::risk_measure::price) == *second->get(kiyosi::risk_measure::price));
        CHECK(std::isfinite(*first->get(kiyosi::risk_measure::price)));
    };
    require_repeatable("european-mc", call, kiyosi::MonteCarloEuropeanEngine{20000, 252, 42});
    require_repeatable("american-mc", put, kiyosi::MonteCarloAmericanEngine{20000, 50, 42});
    require_repeatable("phoenix-mc", phoenix, kiyosi::MonteCarloPhoenixEngine{{1000, 42}});
    require_repeatable("snowball-mc", snowball, kiyosi::MonteCarloSnowballEngine{{1000, 42}});
    require_repeatable("binary-snowball-mc", binary, kiyosi::MonteCarloBinarySnowballEngine{{1000, 42}});
    require_repeatable("ternary-snowball-mc", ternary, kiyosi::MonteCarloTernarySnowballEngine{{1000, 42}});
}

TEST_CASE("CPU parity manifest rejects incomplete output tolerances")
{
    std::istringstream input{
        "case_id\tinstrument\tengine\tvariant\tinputs\toutputs\ttolerances\tvalidation\tconvergence\tmonte_carlo\n"
        "broken\tOption\tEngine\tcall\tspot=100;source_revision=08efb5a0f0f308c0ab7c1a82f1ece1bf63b09fd2;source_symbol=test;convention=Actual/365,BSM;reference_kind=analytic;tolerance=0.1\tprice=1;delta=2\tprice=0.1\t-\t-\t-\n"};
    CHECK_THROWS_WITH(kiyosi::test::parse_parity_cases(input), Catch::Matchers::ContainsSubstring("matching keys"));
}

TEST_CASE("CPU parity manifest requires complete reference provenance")
{
    std::istringstream input{
        "case_id\tinstrument\tengine\tvariant\tinputs\toutputs\ttolerances\tvalidation\tconvergence\tmonte_carlo\n"
        "broken\tEuropeanOption\tAnalyticEuropeanEngine\tcall\tspot=100\tprice=1\tprice=0.1\t-\t-\t-\n"};
    CHECK_THROWS_WITH(kiyosi::test::parse_parity_cases(input),
                      Catch::Matchers::ContainsSubstring("source_revision"));
}
