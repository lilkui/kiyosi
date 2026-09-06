#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_string.hpp>

#include <filesystem>
#include <fstream>
#include <ranges>
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

TEST_CASE("CPU parity manifest rejects incomplete output tolerances")
{
    std::istringstream input{
        "case_id\tinstrument\tengine\tvariant\tinputs\toutputs\ttolerances\tvalidation\tconvergence\tmonte_carlo\n"
        "broken\tOption\tEngine\tcall\tspot=100\tprice=1;delta=2\tprice=0.1\t-\t-\t-\n"};
    CHECK_THROWS_WITH(kiyosi::test::parse_parity_cases(input), Catch::Matchers::ContainsSubstring("matching keys"));
}
