#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_string.hpp>

#include <array>
#include <ranges>
#include <set>
#include <sstream>
#include <string>
#include <utility>
#include <vector>

#include "support/reference_harness.hpp"

using kiyosi::test::fixture_text;

TEST_CASE("QuantLib fixture parser rejects missing or invalid Greek declarations")
{
    const auto original = fixture_text();
    for (const auto identifier : {"ql-european-call-100-1d\t",
                                  "ql-digital-cash-call-100-1d-analyticdigitalengine\t",
                                  "ql-digital-asset-put-100-1d-analyticdigitalengine\t",
                                  "ql-barrier-call-up-and-out-at-expiry-110-1d-analyticbarrierengine\t",
                                  "ql-binary-cash-call-up-and-in-at-expiry-110-100-1d\t"}) {
        const auto start = original.find(identifier);
        REQUIRE(start != std::string::npos);
        const auto end = original.find('\n', start);
        const auto row = original.substr(start, end - start);
        for (const auto& [from, to] : std::vector<std::pair<std::string, std::string>>{
                 {"unavailable_theta=whole-day stability stencil touches expiry_date;", ""},
                 {"unavailable_theta=whole-day stability stencil touches expiry_date", "unavailable_theta=unknown"},
                 {"unit_vega=price/volatility-pp", "unit_vega=price/volatility"},
                 {"uncertainty_price=0", "uncertainty_price=nan"},
                 {"uncertainty_price=0", "uncertainty_price=10"},
                 {"unit_price=price", "unit_price=price;unavailable_typo=unknown"}}) {
            auto changed = row;
            const auto position = changed.find(from);
            REQUIRE(position != std::string::npos);
            changed.replace(position, from.size(), to);
            auto text = original;
            text.replace(start, row.size(), changed);
            std::istringstream input{text};
            CHECK_THROWS_AS(kiyosi::test::parse_reference_cases(input), kiyosi::test::FixtureParseError);
        }
    }
}

TEST_CASE("American reference fixtures reject unknown measures and exercise boundary omissions")
{
    const auto original = fixture_text();
    const auto start = original.find("ql-american-call-exercise-start-crrengine\t");
    REQUIRE(start != std::string::npos);
    const auto end = original.find('\n', start);
    const auto row = original.substr(start, end - start);
    for (const auto& [from, to] : std::vector<std::pair<std::string, std::string>>{
             {"unavailable_theta=whole-day stability stencil precedes exercise window;", ""},
             {"unavailable_theta=whole-day stability stencil precedes exercise window", "unavailable_theta=unknown"},
             {"unit_price=price", "unit_price=price;unavailable_typo=unknown"},
             {"\tdelta=", "\ttypo="}}) {
        auto changed = row;
        const auto position = changed.find(from);
        REQUIRE(position != std::string::npos);
        changed.replace(position, from.size(), to);
        auto text = original;
        text.replace(start, row.size(), changed);
        std::istringstream input{text};
        CHECK_THROWS_AS(kiyosi::test::parse_reference_cases(input), kiyosi::test::FixtureParseError);
    }
}

TEST_CASE("Asian fixtures reject missing boundary declarations and unknown measures")
{
    const auto original = fixture_text();
    for (const auto& identifier : {"ql-asian-geometric-call-100-365d-0elapsed", "ql-asian-arithmetic-call-expiry_date-100"}) {
        const auto start = original.find(std::string{identifier} + '\t');
        REQUIRE(start != std::string::npos);
        const auto row = original.substr(start, original.find('\n', start) - start);
        const auto declaration = row.find("unavailable_theta=");
        REQUIRE(declaration != std::string::npos);
        auto changed = row;
        changed.erase(declaration, changed.find(';', declaration) - declaration + 1);
        auto text = original;
        text.replace(start, row.size(), changed);
        std::istringstream missing{text};
        CHECK_THROWS_AS(kiyosi::test::parse_reference_cases(missing), kiyosi::test::FixtureParseError);
        changed = row;
        const auto output = changed.find("\tprice=");
        REQUIRE(output != std::string::npos);
        changed.replace(output, 7, "\ttypo=");
        text = original;
        text.replace(start, row.size(), changed);
        std::istringstream unknown{text};
        CHECK_THROWS_AS(kiyosi::test::parse_reference_cases(unknown), kiyosi::test::FixtureParseError);
    }
}

TEST_CASE("Binary boundary declarations reject missing or invented sensitivities")
{
    const auto original = fixture_text();
    for (const auto& [identifier, reason] : std::array{
             std::pair{"ql-binary-cash-none-up-and-in-at-expiry-130-100-365d\t", "spot equals barrier: hit-state boundary"},
             std::pair{"ql-binary-cash-call-up-and-in-at-expiry-100-100-0d\t", "terminal payoff: no smooth sensitivities"}}) {
        const auto start = original.find(identifier);
        REQUIRE(start != std::string::npos);
        const auto end = original.find('\n', start);
        const auto row = original.substr(start, end - start);
        for (const auto& [from, to] : std::vector<std::pair<std::string, std::string>>{
                 {"unavailable_delta=" + std::string{reason} + ";", ""},
                 {"unavailable_delta=" + std::string{reason}, "unavailable_delta=unknown"},
                 {"unit_price=price", "unit_price=price;unavailable_typo=unknown"}}) {
            auto changed = row;
            const auto position = changed.find(from);
            REQUIRE(position != std::string::npos);
            changed.replace(position, from.size(), to);
            auto text = original;
            text.replace(start, row.size(), changed);
            std::istringstream input{text};
            CHECK_THROWS_AS(kiyosi::test::parse_reference_cases(input), kiyosi::test::FixtureParseError);
        }
    }
}

TEST_CASE("Reference fixture parser reports malformed rows")
{
    auto text = fixture_text();
    const auto output = text.find("\tprice=");
    REQUIRE(output != std::string::npos);
    text.replace(output + 1, text.find_first_of(";\t\n", output + 1) - output - 1, "price=oops");
    auto parse = [&] {
        std::istringstream input{text};
        return kiyosi::test::parse_reference_cases(input);
    };
    CHECK_THROWS_WITH(parse(), Catch::Matchers::ContainsSubstring("fixture row"));
    CHECK_THROWS_WITH(parse(), Catch::Matchers::ContainsSubstring("invalid outputs"));

    text = fixture_text();
    const auto tolerance = text.find("\tprice=", text.find('\t', text.find("\tprice=") + 1));
    REQUIRE(tolerance != std::string::npos);
    text.replace(tolerance + 1, text.find_first_of(";\t\n", tolerance + 1) - tolerance - 1, "price=-1");
    CHECK_THROWS_WITH(parse(), Catch::Matchers::ContainsSubstring("non-negative"));

    CHECK_THROWS_WITH(kiyosi::test::detail::calendar_date("2025-0x-06", 0, "valuation"),
                      Catch::Matchers::ContainsSubstring("valuation"));
}

TEST_CASE("Reference fixture conversions retain field diagnostics for format and range errors")
{
    for (const std::string text : {"oops", "1e999"}) {
        CHECK_THROWS_WITH(kiyosi::test::detail::number(text, 7, "spot"),
                          "fixture row 7: invalid spot '" + text + "'");
        CHECK_THROWS_WITH(kiyosi::test::detail::numeric_attributes("price=" + text, 7, "outputs"),
                          "fixture row 7: invalid outputs value '" + text + "'");
    }
    const auto parse = [](const std::string& tolerance, const std::string& settings) {
        std::istringstream input{
            "case_id\tinstrument\tengine\tvariant\tinputs\toutputs\ttolerances\n"
            "test\tOption\tMonteCarloEuropeanEngine\tcall\tsource_revision=test;source_symbol=test;convention=test;"
            "reference_kind=statistical;tolerance=" + tolerance +
            ";" + settings + "\tprice=1\tprice=0.1\n"};
        return kiyosi::test::parse_reference_cases(input);
    };
    for (const std::string text : {"oops", "1e999"})
        CHECK_THROWS_WITH(parse(text, "seed=42;paths=32;steps=3"),
                          "fixture row 2: provenance tolerance must be finite and non-negative");
    for (const std::string text : {"oops", "18446744073709551616", "-1", "1.5", "42x"})
        CHECK_THROWS_WITH(parse("0.1", "seed=" + text + ";paths=32;steps=3"),
                          "fixture row 2: invalid seed '" + text + "'");
    const auto valid = parse("0.1", "seed=18446744073709551615;paths=32;steps=3");
    REQUIRE(valid.size() == 1);
    CHECK(kiyosi::test::detail::integer<std::uint64_t>(valid[0].inputs.at("seed"), 2, "seed") == UINT64_MAX);
    for (const std::string settings : {"paths=32;steps=3", "seed=42;steps=3", "seed=42;paths=32",
                                       "seed=42;paths=0;steps=3", "seed=42;paths=32;steps=-1",
                                       "seed=42;paths=1.5;steps=3", "seed=42;paths=32;steps=3x",
                                       "seed=42;paths=2147483648;steps=3", "seed=42;paths=32;steps=2147483648"})
        CHECK_THROWS_AS(parse("0.1", settings), kiyosi::test::FixtureParseError);
}

TEST_CASE("Pricing reference manifest covers instruments, engines, and numerical metadata")
{
    const auto cases = kiyosi::test::load_reference_cases(kiyosi::test::fixture_path());
    REQUIRE(cases.size() == 664);
    std::vector<std::string> names;
    names.reserve(cases.size());
    for (const auto& value : cases) {
        INFO("case=" << value.case_id << " instrument=" << value.instrument << " engine=" << value.engine);
        REQUIRE(value.case_id.starts_with("ql-"));
        REQUIRE(value.inputs.at("owner") == "QuantLib");
        REQUIRE_FALSE(value.instrument.empty());
        REQUIRE_FALSE(value.engine.empty());
        REQUIRE_FALSE(value.variant.empty());
        CHECK_FALSE(value.provenance.source_symbol.empty());
        CHECK_FALSE(value.provenance.convention.empty());
        CHECK((value.provenance.reference_kind == "analytic" ||
               value.provenance.reference_kind == "approximate" ||
               value.provenance.reference_kind == "discretized" ||
               value.provenance.reference_kind == "statistical"));
        REQUIRE(value.outputs.size() == value.tolerances.size());
        for (const auto& [name, tolerance] : value.tolerances) {
            REQUIRE(value.outputs.contains(name));
            CHECK(tolerance >= 0.0);
            names.push_back(value.instrument + "/" + value.engine);
        }
    }
    CHECK(std::ranges::any_of(names, [](const auto& name) { return name == "BarrierOption/FiniteDifferenceBarrierEngine"; }));
}

TEST_CASE("Pricing reference manifest inventories QuantLib supported engines and contracts")
{
    const auto cases = kiyosi::test::load_reference_cases(kiyosi::test::fixture_path());
    const std::set<std::string> required_engines{
        "AnalyticBarrierEngine", "AnalyticBinaryBarrierEngine", "AnalyticDigitalEngine",
        "AnalyticEuropeanEngine", "TurnbullWakemanArithmeticAveragePriceEngine",
        "BjerksundStenslandAmericanEngine", "CrrEngine",
        "FiniteDifferenceAmericanEngine", "FiniteDifferenceBarrierEngine",
        "FiniteDifferenceDigitalEngine", "FiniteDifferenceEuropeanEngine",
        "AnalyticGeometricAveragePriceEngine", "QuadratureDigitalEngine", "IntegralEuropeanEngine",
        "MonteCarloAmericanEngine", "MonteCarloEuropeanEngine"};
    const std::set<std::string> required_instruments{
        "AmericanOption", "ArithmeticAveragePriceOption", "BarrierOption",
        "BinaryBarrierOption", "TouchOption", "AssetOrNothingOption",
        "CashOrNothingOption", "EuropeanOption", "GeometricAveragePriceOption"};
    const std::set<std::string> required_pairs{
        "AmericanOption/FiniteDifferenceAmericanEngine",
        "AmericanOption/MonteCarloAmericanEngine", "AmericanOption/BjerksundStenslandAmericanEngine",
        "AmericanOption/CrrEngine",
        "ArithmeticAveragePriceOption/TurnbullWakemanArithmeticAveragePriceEngine", "BarrierOption/AnalyticBarrierEngine",
        "BarrierOption/FiniteDifferenceBarrierEngine",
        "BinaryBarrierOption/AnalyticBinaryBarrierEngine", "TouchOption/AnalyticBinaryBarrierEngine",
        "AssetOrNothingOption/AnalyticDigitalEngine",
        "AssetOrNothingOption/QuadratureDigitalEngine", "AssetOrNothingOption/FiniteDifferenceDigitalEngine",
        "CashOrNothingOption/AnalyticDigitalEngine", "CashOrNothingOption/FiniteDifferenceDigitalEngine",
        "CashOrNothingOption/QuadratureDigitalEngine", "EuropeanOption/AnalyticEuropeanEngine",
        "EuropeanOption/CrrEngine",
        "EuropeanOption/FiniteDifferenceEuropeanEngine", "EuropeanOption/IntegralEuropeanEngine",
        "EuropeanOption/MonteCarloEuropeanEngine", "GeometricAveragePriceOption/AnalyticGeometricAveragePriceEngine"};
    std::set<std::string> actual_engines;
    std::set<std::string> actual_instruments;
    std::set<std::string> actual_pairs;
    bool has_settlement = false;
    bool has_monitoring = false;
    for (const auto& value : cases) {
        actual_engines.insert(value.engine);
        actual_instruments.insert(value.instrument);
        actual_pairs.insert(value.instrument + "/" + value.engine);
        has_settlement |= value.inputs.contains("settlement");
        has_monitoring |= value.inputs.contains("monitoring");
        REQUIRE(value.inputs.at("calendar") == "weekends_only");
        REQUIRE(value.outputs.contains("price"));
    }
    CHECK(actual_engines == required_engines);
    CHECK(actual_instruments == required_instruments);
    CHECK(actual_pairs == required_pairs);
    CHECK(has_settlement);
    CHECK(has_monitoring);
}

TEST_CASE("Pricing reference manifest rejects incomplete output tolerances")
{
    std::istringstream input{
        "case_id\tinstrument\tengine\tvariant\tinputs\toutputs\ttolerances\n"
        "broken\tOption\tEngine\tcall\tspot=100;source_revision=test;source_symbol=test;convention=Actual/365,BSM;reference_kind=analytic;tolerance=0.1\tprice=1;delta=2\tprice=0.1\n"};
    CHECK_THROWS_WITH(kiyosi::test::parse_reference_cases(input), Catch::Matchers::ContainsSubstring("matching keys"));
}

TEST_CASE("Pricing reference manifest requires complete reference provenance")
{
    std::istringstream input{
        "case_id\tinstrument\tengine\tvariant\tinputs\toutputs\ttolerances\n"
        "broken\tEuropeanOption\tAnalyticEuropeanEngine\tcall\tspot=100\tprice=1\tprice=0.1\n"};
    CHECK_THROWS_WITH(kiyosi::test::parse_reference_cases(input),
                      Catch::Matchers::ContainsSubstring("source_revision"));
}
