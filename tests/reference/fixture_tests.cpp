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

#include "support/reference_fixture.hpp"

namespace {

const std::map<std::string, kiyosi::risk_measure> measures{
        {"price", kiyosi::risk_measure::price}, {"delta", kiyosi::risk_measure::delta},
        {"gamma", kiyosi::risk_measure::gamma}, {"speed", kiyosi::risk_measure::speed},
        {"theta", kiyosi::risk_measure::theta}, {"charm", kiyosi::risk_measure::charm},
        {"color", kiyosi::risk_measure::color}, {"vega", kiyosi::risk_measure::vega},
        {"vanna", kiyosi::risk_measure::vanna}, {"zomma", kiyosi::risk_measure::zomma},
        {"rho", kiyosi::risk_measure::rho}};

std::filesystem::path fixture_path()
{
    return std::filesystem::path{KIYOSI_SOURCE_DIR} / "tests" / "fixtures" / "pricing_reference.tsv";
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
void check_price(const kiyosi::test::ReferenceCase& fixture, const PriceResult& priced)
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

TEST_CASE("QuantLib generated references validate all Greeks and boundary declarations")
{
    const auto cases = kiyosi::test::load_reference_cases(fixture_path());
    std::set<std::string> identifiers;
    std::size_t compared = 0;
    for (const auto& fixture : cases) {
        INFO("case=" << fixture.case_id);
        REQUIRE(identifiers.insert(fixture.case_id).second);
        const auto& inputs = fixture.inputs;
        const bool owned = inputs.contains("owner") && inputs.at("owner") == "QuantLib";
        REQUIRE(owned == fixture.case_id.starts_with("ql-"));
        if (!owned) continue;
        REQUIRE(fixture.instrument == "EuropeanOption");
        REQUIRE(fixture.engine == "AnalyticEuropeanEngine");
        REQUIRE(fixture.case_id.starts_with("ql-european-"));

        REQUIRE(fixture.outputs.contains("price"));
        REQUIRE_FALSE(fixture.validation.has_value());
        REQUIRE_FALSE(fixture.convergence.has_value());
        REQUIRE_FALSE(fixture.monte_carlo.has_value());
        REQUIRE(fixture.provenance.convention == "Actual/365 Fixed, continuously compounded BSM");
        REQUIRE(fixture.provenance.reference_kind == "analytic");
        REQUIRE(fixture.provenance.explicit_tolerance == fixture.tolerances.at("price"));
        const auto number = [&](const std::string& key) {
            std::size_t index = 0;
            return kiyosi::test::detail::number({inputs.at(key)}, index, 0, key);
        };
        const auto date = [&](const std::string& key) {
            std::size_t index = 0;
            return kiyosi::test::detail::calendar_date({inputs.at(key)}, index, 0, key);
        };
        REQUIRE((inputs.at("option") == "call" || inputs.at("option") == "put"));
        REQUIRE(fixture.variant == inputs.at("option"));
        REQUIRE(date("effective") <= date("valuation"));
        REQUIRE(date("valuation") < date("expiry"));
        const auto option = kiyosi::make_european_option(
            inputs.at("option") == "call" ? kiyosi::option_type::call : kiyosi::option_type::put,
            number("strike"), date("effective"), date("expiry"));
        REQUIRE(option.has_value());
        const auto parameters = kiyosi::make_bsm_parameters(number("rate"), number("dividend"), number("volatility"));
        REQUIRE(parameters.has_value());
        const auto spot = kiyosi::make_asset_price(number("spot"));
        REQUIRE(spot.has_value());
        const auto context = kiyosi::make_pricing_context(*parameters, *spot, date("valuation"));
        REQUIRE(context.has_value());
        const kiyosi::AnalyticEuropeanEngine engine;
        const auto native = engine.price(*option, *context);
        check_price(fixture, native);
        const bool boundary = (date("expiry") - date("valuation")).count() <= 2;
        const auto numerical = kiyosi::NumericalAnalyticsEngine{engine}.price(*option, *context);
        REQUIRE(numerical.has_value());
        std::size_t available = 0;
        for (const auto& [name, measure] : measures) {
            INFO("measure=" << name);
            const bool unavailable = boundary && (name == "theta" || name == "charm" || name == "color");
            REQUIRE(inputs.contains("unit_" + name));
            REQUIRE(inputs.contains("unavailable_" + name) == unavailable);
            REQUIRE(fixture.outputs.contains(name) != unavailable);
            if (unavailable) {
                REQUIRE(inputs.at("unavailable_" + name) == "whole-day stability stencil touches expiry");
                continue;
            }
            ++available;
            REQUIRE(native->get(measure).has_value());
            const double expected = fixture.outputs.at(name);
            CHECK_THAT(*native->get(measure), Catch::Matchers::WithinAbs(expected, fixture.tolerances.at(name) + number("uncertainty_" + name)));
            REQUIRE(number("uncertainty_" + name) >= 0);
            REQUIRE(numerical->get(measure).has_value());
            CHECK_THAT(*numerical->get(measure), Catch::Matchers::WithinAbs(expected, number("numerical_tolerance_" + name) + number("uncertainty_" + name)));
        }
        REQUIRE(fixture.outputs.size() == available);
        const auto implied = engine.implied_volatility(*option, *context, fixture.outputs.at("price"));
        INFO("implied volatility: " << (implied ? "ok" : implied.error().message));
        REQUIRE(implied.has_value());
        CHECK_THAT(*implied, Catch::Matchers::WithinAbs(number("volatility"), 1e-7));
        ++compared;
    }
    REQUIRE(compared > 0);
}

TEST_CASE("QuantLib fixture parser rejects missing or invalid Greek declarations")
{
    const auto original = fixture_text();
    const auto start = original.find("ql-european-call-100-1d\t");
    REQUIRE(start != std::string::npos);
    const auto end = original.find('\n', start);
    const auto row = original.substr(start, end - start);
    for (const auto& [from, to] : std::vector<std::pair<std::string, std::string>>{
             {"unavailable_theta=whole-day stability stencil touches expiry;", ""},
             {"unavailable_theta=whole-day stability stencil touches expiry", "unavailable_theta=unknown"},
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

TEST_CASE("Reference fixture parser reports malformed rows")
{
    auto text = fixture_text();
    const auto output = text.find("\tprice=13.151137");
    REQUIRE(output != std::string::npos);
    text.replace(output + 1, std::string{"price=13.151137"}.size(), "price=oops");
    auto parse = [&] {
        std::istringstream input{text};
        return kiyosi::test::parse_reference_cases(input);
    };
    CHECK_THROWS_WITH(parse(), Catch::Matchers::ContainsSubstring("fixture row 5"));
    CHECK_THROWS_WITH(parse(), Catch::Matchers::ContainsSubstring("invalid outputs"));

    text = fixture_text();
    const auto tolerance = text.find("price=0.00001");
    REQUIRE(tolerance != std::string::npos);
    text.replace(tolerance, std::string{"price=0.00001"}.size(), "price=-1");
    CHECK_THROWS_WITH(parse(), Catch::Matchers::ContainsSubstring("non-negative"));

    std::size_t index = 0;
    CHECK_THROWS_WITH(kiyosi::test::detail::calendar_date({"2025-0x-06"}, index, 0, "valuation"),
                      Catch::Matchers::ContainsSubstring("valuation"));
}

TEST_CASE("Reference fixture tolerances are inclusive and mismatch reports are useful")
{
    CHECK(kiyosi::test::within_tolerance(2.0, 1.0, 1.0));
    CHECK_FALSE(kiyosi::test::within_tolerance(2.000001, 1.0, 1.0));

    auto fixture = kiyosi::test::load_reference_cases(fixture_path()).front();
    const auto actual = fixture.outputs;
    CHECK(kiyosi::test::compare_fixture(fixture, actual).empty());
    fixture.outputs.at("price") += 1.0;
    const auto failures = kiyosi::test::compare_fixture(fixture, actual);
    REQUIRE(failures.size() == 1);
    const auto message = failures.front().message();
    CHECK(message.find("case='european-analytic'") != std::string::npos);
    CHECK(message.find("output='price'") != std::string::npos);
    CHECK(message.find("expected=") != std::string::npos);
    CHECK(message.find("actual=") != std::string::npos);
    CHECK(message.find("tolerance=") != std::string::npos);
}

TEST_CASE("Pricing reference manifest covers instruments, engines, and numerical metadata")
{
    const auto cases = kiyosi::test::load_reference_cases(fixture_path());
    REQUIRE(cases.size() >= 20);
    std::vector<std::string> names;
    names.reserve(cases.size());
    for (const auto& value : cases) {
        INFO("case=" << value.case_id << " instrument=" << value.instrument << " engine=" << value.engine);
        REQUIRE_FALSE(value.case_id.empty());
        REQUIRE_FALSE(value.instrument.empty());
        REQUIRE_FALSE(value.engine.empty());
        REQUIRE_FALSE(value.variant.empty());
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

TEST_CASE("Pricing reference manifest closes every concrete engine and contract variant")
{
    const auto cases = kiyosi::test::load_reference_cases(fixture_path());
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

TEST_CASE("European reference fixtures compare price, every Greek, and implied volatility")
{
    const auto cases = kiyosi::test::load_reference_cases(fixture_path());

    for (const auto* id : {"european-analytic", "european-analytic-put"}) {
        const auto fixture = std::ranges::find_if(cases, [&](const auto& value) { return value.case_id == id; });
        REQUIRE(fixture != cases.end());
        INFO("case=" << fixture->case_id);
        const auto& inputs = fixture->inputs;
        const auto number = [&](const std::string& key) {
            std::size_t index = 0;
            return kiyosi::test::detail::number({inputs.at(key)}, index, 0, key);
        };
        const auto date = [&](const std::string& key) {
            std::size_t index = 0;
            return kiyosi::test::detail::calendar_date({inputs.at(key)}, index, 0, key);
        };
        REQUIRE((inputs.at("option") == "call" || inputs.at("option") == "put"));
        const auto option = kiyosi::make_european_option(
            inputs.at("option") == "call" ? kiyosi::option_type::call : kiyosi::option_type::put,
            number("strike"), date("expiry"));
        REQUIRE(option.has_value());
        const auto parameters = kiyosi::make_bsm_parameters(number("rate"), number("dividend"), number("volatility"));
        REQUIRE(parameters.has_value());
        const auto spot = kiyosi::make_asset_price(number("spot"));
        REQUIRE(spot.has_value());
        const auto context = kiyosi::make_pricing_context(*parameters, *spot, date("valuation"));
        REQUIRE(context.has_value());
        const kiyosi::AnalyticEuropeanEngine engine;
        const auto result = engine.price(*option, *context);
        REQUIRE(result.has_value());
        const auto implied = engine.implied_volatility(*option, *context, number("observed_price"));
        REQUIRE(implied.has_value());
        std::map<std::string, double> actual{{"implied_volatility", *implied}};
        for (const auto& [name, measure] : measures) {
            REQUIRE(result->get(measure).has_value());
            actual.emplace(name, *result->get(measure));
        }
        REQUIRE(fixture->outputs.size() == actual.size());
        for (const auto& [name, value] : actual) {
            REQUIRE(fixture->outputs.contains(name));
        }
        kiyosi::test::check_fixture(*fixture, actual);
    }
}

TEST_CASE("Vanilla and digital engines match reviewed pricing fixtures")
{
    const auto cases = kiyosi::test::load_reference_cases(fixture_path());
    const auto context = standard_context();
    const auto expiry = standard_expiry();
    const auto call = *kiyosi::make_european_call(100.0, context.valuation_date(), expiry);
    const auto put = *kiyosi::make_european_put(100.0, context.valuation_date(), expiry);
    const auto find = [&](std::string_view case_id) -> const kiyosi::test::ReferenceCase& {
        const auto found = std::ranges::find_if(cases, [&](const auto& value) { return value.case_id == case_id; });
        REQUIRE(found != cases.end());
        return *found;
    };

    check_price(find("european-analytic"), kiyosi::AnalyticEuropeanEngine{}.price(call, context));
    check_price(find("european-binomial"), kiyosi::BinomialEuropeanEngine{200}.price(call, context));
    check_price(find("crr-vanilla"), kiyosi::CrrEngine{200}.price(call, context));
    check_price(find("european-integral"), kiyosi::IntegralEuropeanEngine{}.price(put, context));
    check_price(find("european-fd"), kiyosi::FiniteDifferenceEuropeanEngine{
        {200, 4000, kiyosi::finite_difference_scheme::explicit_euler}}.price(call, context));
    const auto cash_call = *kiyosi::make_cash_or_nothing_option(kiyosi::option_type::call, 100.0, 10.0, expiry);
    const auto asset_put = *kiyosi::make_asset_or_nothing_option(kiyosi::option_type::put, 100.0, expiry);
    check_price(find("cash-digital-analytic"), kiyosi::AnalyticDigitalEngine{}.price(cash_call, context));
    check_price(find("asset-digital-analytic"), kiyosi::AnalyticDigitalEngine{}.price(asset_put, context));
    check_price(find("digital-integral"), kiyosi::IntegralDigitalEngine{}.price(cash_call, context));
    const auto digital_put = *kiyosi::make_cash_or_nothing_option(kiyosi::option_type::put, 100.0, 10.0, expiry);
    check_price(find("digital-fd"), kiyosi::FiniteDifferenceDigitalEngine{200, 200}.price(digital_put, context));
}

TEST_CASE("Barrier fixtures reconstruct every pinned public variant")
{
    const auto cases = kiyosi::test::load_reference_cases(fixture_path());
    const auto valuation = standard_context().valuation_date();
    const auto expiry = standard_expiry();
    const auto context = standard_context();
    const auto find = [&](std::string_view id) -> const auto& {
        const auto found = std::ranges::find_if(cases, [&](const auto& value) { return value.case_id == id; });
        REQUIRE(found != cases.end());
        return *found;
    };
    const auto barrier_kind = [](std::string_view value) {
        if (value == "up_and_in") return kiyosi::barrier_type::up_and_in;
        if (value == "up_and_out") return kiyosi::barrier_type::up_and_out;
        if (value == "down_and_in") return kiyosi::barrier_type::down_and_in;
        return kiyosi::barrier_type::down_and_out;
    };
    const auto observations = [&](const auto& input) {
        std::vector<kiyosi::date> dates;
        if (input.contains("interval_days"))
            for (auto date = valuation + std::chrono::days{std::stoi(input.at("interval_days"))};
                 date <= expiry; date += std::chrono::days{std::stoi(input.at("interval_days"))})
                dates.push_back(date);
        return dates;
    };
    const auto vanilla = [&](std::string_view id) {
        const auto& fixture = find(id);
        const auto& input = fixture.inputs;
        const auto option = *kiyosi::make_barrier_option(
            input.at("option") == "call" ? kiyosi::option_type::call : kiyosi::option_type::put,
            std::stod(input.at("strike")), valuation, expiry, std::stod(input.at("barrier")),
            barrier_kind(input.at("barrier_kind")), std::stod(input.at("rebate")),
            input.at("settlement") == "at_hit" ? kiyosi::rebate_timing::at_hit : kiyosi::rebate_timing::at_expiry,
            input.contains("monitoring") && input.at("monitoring") == "scheduled"
                ? kiyosi::observation_mode::scheduled : kiyosi::observation_mode::continuous,
            observations(input));
        check_price(fixture, kiyosi::AnalyticBarrierEngine{}.price(option, context));
    };
    for (const auto id : {"barrier-down-in-call", "barrier-down-in-put", "barrier-up-in-call",
                          "barrier-up-in-put", "barrier-down-out-call", "barrier-down-out-put",
                          "barrier-up-out-call", "barrier-up-out-put", "barrier-scheduled-monitoring"})
        vanilla(id);
    for (const auto id : {"barrier-fd-down-in-call", "barrier-fd-down-in-put", "barrier-fd-up-in-call",
                          "barrier-fd-up-in-put", "barrier-fd-down-out-call", "barrier-fd-down-out-put",
                          "barrier-fd-up-out-call", "barrier-fd-up-out-put"}) {
        const auto& fixture = find(id);
        const auto& input = fixture.inputs;
        const auto option = *kiyosi::make_barrier_option(
            input.at("option") == "call" ? kiyosi::option_type::call : kiyosi::option_type::put,
            std::stod(input.at("strike")), valuation, expiry, std::stod(input.at("barrier")),
            barrier_kind(input.at("barrier_kind")), std::stod(input.at("rebate")));
        check_price(fixture, kiyosi::FiniteDifferenceBarrierEngine{1000, 1000}.price(option, context));
    }

    const auto binary = [&](std::string_view id) {
        const auto& fixture = find(id);
        const auto& input = fixture.inputs;
        std::optional<kiyosi::option_type> type;
        if (input.contains("option"))
            type = input.at("option") == "call" ? kiyosi::option_type::call : kiyosi::option_type::put;
        const bool asset = input.contains("asset_settlement") && input.at("asset_settlement") == "true";
        const auto option = *kiyosi::make_binary_barrier_option(
            type, std::stod(input.at("strike")), valuation, expiry, std::stod(input.at("barrier")),
            barrier_kind(input.at("barrier_kind")), std::stod(input.at("payout")), asset,
            input.at("settlement") == "at_hit" ? kiyosi::rebate_timing::at_hit : kiyosi::rebate_timing::at_expiry,
            input.contains("monitoring") && input.at("monitoring") == "scheduled"
                ? kiyosi::observation_mode::scheduled : kiyosi::observation_mode::continuous,
            observations(input));
        check_price(fixture, kiyosi::AnalyticBinaryBarrierEngine{}.price(option, context));
    };
    for (const auto id : {"binary-barrier-down-in-cash-hit", "binary-barrier-up-in-cash-hit",
                          "binary-barrier-down-in-asset-hit", "binary-barrier-up-in-asset-hit",
                          "binary-barrier-down-in-cash-expiry", "binary-barrier-up-in-cash-expiry",
                          "binary-barrier-down-in-asset-expiry", "binary-barrier-up-in-asset-expiry",
                          "binary-barrier-down-out-cash-expiry", "binary-barrier-up-out-cash-expiry",
                          "binary-barrier-down-out-asset-expiry", "binary-barrier-up-out-asset-expiry",
                          "binary-barrier-down-in-call-cash", "binary-barrier-up-in-call-cash",
                          "binary-barrier-down-in-call-asset", "binary-barrier-up-in-call-asset",
                          "binary-barrier-down-in-put-cash", "binary-barrier-up-in-put-cash",
                          "binary-barrier-down-in-put-asset", "binary-barrier-up-in-put-asset",
                          "binary-barrier-down-out-call-cash", "binary-barrier-up-out-call-cash",
                          "binary-barrier-down-out-call-asset", "binary-barrier-up-out-call-asset",
                          "binary-barrier-down-out-put-cash", "binary-barrier-up-out-put-cash",
                          "binary-barrier-down-out-put-asset", "binary-barrier-up-out-put-asset"})
        binary(id);
}

TEST_CASE("Barrier validation fixtures exercise public constructors")
{
    const auto cases = kiyosi::test::load_reference_cases(fixture_path());
    const auto expiry = standard_expiry();
    const auto expected = [&](std::string_view id) {
        const auto found = std::ranges::find_if(cases, [&](const auto& value) { return value.case_id == id; });
        REQUIRE(found != cases.end());
        REQUIRE(found->validation.has_value());
        return found->validation->category;
    };
    const auto category = [](std::string_view value) {
        if (value == "invalid_strike") return kiyosi::error_category::invalid_strike;
        if (value == "invalid_schedule") return kiyosi::error_category::invalid_schedule;
        if (value == "invalid_option") return kiyosi::error_category::invalid_option;
        return kiyosi::error_category::invalid_parameter;
    };
    CHECK(kiyosi::make_barrier_option(kiyosi::option_type::call, -1.0, expiry, 90.0,
                                      kiyosi::barrier_type::down_and_in).error().category ==
          category(expected("invalid-barrier-strike")));
    CHECK(kiyosi::make_barrier_option(kiyosi::option_type::call, 100.0, expiry, 90.0,
                                      kiyosi::barrier_type::down_and_in, 10.0,
                                      kiyosi::rebate_timing::at_hit).error().category ==
          category(expected("invalid-barrier-hit-rebate")));
    CHECK(kiyosi::make_binary_barrier_option(std::nullopt, 100.0, expiry, 90.0,
                                             kiyosi::barrier_type::down_and_out, 10.0, false,
                                             kiyosi::rebate_timing::at_hit).error().category ==
          category(expected("invalid-binary-hit-settlement")));
    CHECK(kiyosi::make_binary_barrier_option(std::nullopt, 100.0, expiry, 90.0,
                                             kiyosi::barrier_type::down_and_in, 10.0, false,
                                             kiyosi::rebate_timing::at_expiry,
                                             kiyosi::observation_mode::scheduled).error().category ==
          category(expected("invalid-binary-schedule")));
}

TEST_CASE("Asian engines match pinned terms and market assumptions")
{
    const auto effective = kiyosi::date{std::chrono::year{2025} / 1 / 6};
    const auto valuation = effective + std::chrono::days{90};
    const auto expiry = effective + std::chrono::days{180};
    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    const auto context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(100.0), valuation);
    const auto cases = kiyosi::test::load_reference_cases(fixture_path());
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

TEST_CASE("Pricing reference public properties cover payoff, in-out, convergence, and seeded paths")
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

    const auto cases = kiyosi::test::load_reference_cases(fixture_path());
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
    const auto cases = kiyosi::test::load_reference_cases(fixture_path());
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
    const auto cases = kiyosi::test::load_reference_cases(fixture_path());
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

/*
    const auto cases = kiyosi::test::load_reference_cases(
        std::filesystem::path{KIYOSI_SOURCE_DIR} / "tests/fixtures/structured_reference.tsv");
    REQUIRE_FALSE(cases.empty());
    std::set<std::string> engines;
    for (const auto& fixture : cases) {
        INFO(fixture.case_id);
        const auto& inputs = fixture.inputs;
        const auto number = [&](const std::string& key) { return std::stod(inputs.at(key)); };
        const auto parse_date = [](const std::string& value) {
            std::size_t index = 0;
            return kiyosi::test::detail::calendar_date({value}, index, 0, "structured date");
        };
        const auto effective = parse_date(inputs.at("effective"));
        const auto expiry = parse_date(inputs.at("expiry"));
        const auto valuation = parse_date(inputs.at("valuation"));
        const auto calendar = inputs.at("calendar") == "sse" ? kiyosi::sse_calendar() : kiyosi::all_days_calendar();
        const auto parameters = kiyosi::make_bsm_parameters(number("rate"), number("dividend"), number("volatility"));
        REQUIRE(parameters.has_value());
        const auto context = kiyosi::make_pricing_context(*parameters, *kiyosi::make_asset_price(number("spot")), valuation, calendar);
        REQUIRE(context.has_value());
        const auto check = [&](const auto& instrument) {
            REQUIRE(instrument.has_value());
            using Instrument = typename std::remove_cvref_t<decltype(instrument)>::value_type;
            if (fixture.monte_carlo) {
                const auto& budget = *fixture.monte_carlo;
                std::size_t steps = 0;
                for (auto current = valuation + std::chrono::days{1}; current <= expiry; current += std::chrono::days{1})
                    steps += calendar.is_trading_day(current) ? 1 : 0;
                CHECK(budget.steps == steps);
                const kiyosi::MonteCarloStructuredEngine<Instrument> engine{
                    {static_cast<int>(budget.paths), budget.seed}};
                check_price(fixture, engine.price(*instrument, *context));
            } else {
                REQUIRE(inputs.at("scheme") == "crank_nicolson");
                const kiyosi::FiniteDifferenceStructuredEngine<Instrument> engine{{
                    std::stoi(inputs.at("asset_steps")), std::stoi(inputs.at("time_steps")),
                    kiyosi::finite_difference_scheme::crank_nicolson, number("upper_boundary")}};
                check_price(fixture, engine.price(*instrument, *context));
            }
        };
        engines.insert(fixture.engine);
        if (fixture.instrument == "Accumulator") {
            check(kiyosi::make_accumulator(number("strike"), number("knock_out"), number("daily_quantity"),
                number("acceleration"), number("accumulated_quantity"), effective, expiry));
            continue;
        }
        const auto numbers = [&](const std::string& key) {
            std::vector<double> values;
            for (const auto& value : kiyosi::test::detail::split(inputs.at(key), ',')) values.push_back(std::stod(value));
            return values;
        };
        std::vector<kiyosi::date> observations;
        for (const auto& value : kiyosi::test::detail::split(inputs.at("observations"), ',')) observations.push_back(parse_date(value));
        const auto touch = inputs.at("touch") == "down" ? kiyosi::barrier_touch_status::down :
                           inputs.at("touch") == "up" ? kiyosi::barrier_touch_status::up : kiyosi::barrier_touch_status::none;
        const auto frequency = inputs.contains("frequency") && inputs.at("frequency") == "at_expiry" ?
            kiyosi::observation_frequency::at_expiry : kiyosi::observation_frequency::daily;
        if (fixture.instrument == "PhoenixOption")
            check(kiyosi::make_phoenix_option(number("coupon_rate"), number("initial_price"), number("knock_in_price"),
                numbers("knock_out_prices"), numbers("coupon_barriers"), number("upper_strike"), number("lower_strike"),
                observations, frequency, touch, number("principal_ratio"), effective, expiry));
        else if (fixture.instrument == "SnowballOption")
            check(kiyosi::make_snowball_option(numbers("knock_out_coupon_rates"), number("maturity_coupon_rate"),
                number("initial_price"), number("knock_in_price"), numbers("knock_out_prices"), number("upper_strike"),
                number("lower_strike"), observations, frequency, touch, number("principal_ratio"), effective, expiry));
        else if (fixture.instrument == "BinarySnowballOption")
            check(kiyosi::make_binary_snowball_option(numbers("knock_out_coupon_rates"), number("maturity_coupon_rate"),
                number("initial_price"), numbers("knock_out_prices"), number("upper_strike"), number("lower_strike"),
                observations, touch, number("principal_ratio"), effective, expiry));
        else {
            REQUIRE(fixture.instrument == "TernarySnowballOption");
            check(kiyosi::make_ternary_snowball_option(numbers("knock_out_coupon_rates"), number("maturity_coupon_rate"),
                number("minimal_coupon_rate"), number("initial_price"), number("knock_in_price"), numbers("knock_out_prices"),
                number("upper_strike"), number("lower_strike"), observations, frequency, touch, number("principal_ratio"), effective, expiry));
        }
    }
    CHECK(engines.size() == 10);
*/

TEST_CASE("Pricing reference manifest rejects incomplete output tolerances")
{
    std::istringstream input{
        "case_id\tinstrument\tengine\tvariant\tinputs\toutputs\ttolerances\tvalidation\tconvergence\tmonte_carlo\n"
        "broken\tOption\tEngine\tcall\tspot=100;source_revision=08efb5a0f0f308c0ab7c1a82f1ece1bf63b09fd2;source_symbol=test;convention=Actual/365,BSM;reference_kind=analytic;tolerance=0.1\tprice=1;delta=2\tprice=0.1\t-\t-\t-\n"};
    CHECK_THROWS_WITH(kiyosi::test::parse_reference_cases(input), Catch::Matchers::ContainsSubstring("matching keys"));
}

TEST_CASE("Pricing reference manifest requires complete reference provenance")
{
    std::istringstream input{
        "case_id\tinstrument\tengine\tvariant\tinputs\toutputs\ttolerances\tvalidation\tconvergence\tmonte_carlo\n"
        "broken\tEuropeanOption\tAnalyticEuropeanEngine\tcall\tspot=100\tprice=1\tprice=0.1\t-\t-\t-\n"};
    CHECK_THROWS_WITH(kiyosi::test::parse_reference_cases(input),
                      Catch::Matchers::ContainsSubstring("source_revision"));
}
