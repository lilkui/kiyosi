#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_string.hpp>

#include <filesystem>
#include <fstream>
#include <sstream>
#include <string>

#include "parity_fixture.hpp"

namespace {

std::filesystem::path fixture_path()
{
    return std::filesystem::path{ITO_SOURCE_DIR} / "tests" / "fixtures" / "european_bsm.tsv";
}

std::string fixture_text()
{
    std::ifstream input(fixture_path());
    REQUIRE(input.good());
    return {std::istreambuf_iterator<char>{input}, std::istreambuf_iterator<char>{}};
}

}

TEST_CASE("European parity fixtures compare value, Greeks, and implied volatility")
{
    const auto fixtures = ito::test::load_parity_fixtures(fixture_path());
    REQUIRE(fixtures.size() == 2);
    const ito::AnalyticEuropeanEngine engine;
    for (const auto& fixture : fixtures) {
        const auto option = ito::make_european_option(fixture.option, fixture.strike, fixture.expiry);
        REQUIRE(option.has_value());
        const auto parameters = ito::make_bsm_parameters(
            fixture.risk_free_rate, fixture.dividend_yield, fixture.volatility);
        REQUIRE(parameters.has_value());
        const auto context = ito::make_pricing_context(
            *parameters, *ito::make_asset_price(fixture.spot), fixture.valuation_date);
        REQUIRE(context.has_value());
        const auto priced = engine.price(*option, *context);
        REQUIRE(priced.has_value());
        const auto implied = engine.implied_volatility(*option, *context, fixture.observed_price);
        REQUIRE(implied.has_value());
        ito::test::check_fixture(fixture, *priced, *implied);
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
        return ito::test::parse_parity_fixtures(input);
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
    CHECK(ito::test::within_tolerance(2.0, 1.0, 1.0));
    CHECK_FALSE(ito::test::within_tolerance(2.000001, 1.0, 1.0));

    auto fixture = ito::test::load_parity_fixtures(fixture_path()).front();
    fixture.value += 1.0;
    const ito::PricingResult actual{fixture.value - 1.0, fixture.delta, fixture.gamma, fixture.speed,
                                    fixture.theta, fixture.charm, fixture.color, fixture.vega,
                                    fixture.vanna, fixture.zomma, fixture.rho};
    const auto failures = ito::test::compare_fixture(fixture, actual, fixture.implied_volatility);
    REQUIRE_FALSE(failures.empty());
    const auto message = failures.front().message();
    CHECK(message.find("case='reviewed-call-1'") != std::string::npos);
    CHECK(message.find("output='value'") != std::string::npos);
    CHECK(message.find("expected=") != std::string::npos);
    CHECK(message.find("actual=") != std::string::npos);
    CHECK(message.find("tolerance=") != std::string::npos);
}
