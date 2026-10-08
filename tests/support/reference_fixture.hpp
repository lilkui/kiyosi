#pragma once

#include <array>
#include <charconv>
#include <chrono>
#include <concepts>
#include <cmath>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <map>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include <kiyosi/kiyosi.hpp>

namespace kiyosi::test {

using FixtureAttributes = std::map<std::string, std::string>;

struct ReferenceProvenance {
    std::string source_revision;
    std::string source_symbol;
    std::string convention;
    std::string reference_kind;
    double explicit_tolerance = 0.0;
};

struct ReferenceCase { // NOLINT(bugprone-exception-escape): MSVC map moves may allocate in debug builds.
    std::string case_id;
    std::string instrument;
    std::string engine;
    std::string variant;
    FixtureAttributes inputs;
    ReferenceProvenance provenance;
    std::map<std::string, double> outputs;
    std::map<std::string, double> tolerances;
};

class FixtureParseError : public std::runtime_error {
public:
    using std::runtime_error::runtime_error;
};

namespace detail {

inline std::vector<std::string> split(std::string_view line, char delimiter)
{
    std::vector<std::string> fields;
    std::size_t start = 0;
    while (start <= line.size()) {
        const auto end = line.find(delimiter, start);
        fields.emplace_back(line.substr(start, end == std::string_view::npos ? end : end - start));
        if (end == std::string_view::npos) break;
        start = end + 1;
    }
    return fields;
}

inline std::string field(const std::vector<std::string>& fields, std::size_t index,
                         std::size_t row, std::string_view name)
{
    if (fields[index].empty()) {
        throw FixtureParseError("fixture row " + std::to_string(row) + ": empty " + std::string{name});
    }
    return fields[index];
}

inline double number(const std::string& text, std::size_t row, std::string_view name)
{
    if (text.empty())
        throw FixtureParseError("fixture row " + std::to_string(row) + ": empty " + std::string{name});
    std::size_t parsed = 0;
    double value = 0.0;
    try {
        value = std::stod(text, &parsed);
    } catch (const std::invalid_argument&) {
        throw FixtureParseError("fixture row " + std::to_string(row) + ": invalid " +
                                std::string{name} + " '" + text + "'");
    } catch (const std::out_of_range&) {
        throw FixtureParseError("fixture row " + std::to_string(row) + ": invalid " +
                                std::string{name} + " '" + text + "'");
    }
    if (parsed != text.size() || !std::isfinite(value)) {
        throw FixtureParseError("fixture row " + std::to_string(row) + ": invalid " +
                                std::string{name} + " '" + text + "'");
    }
    return value;
}

template <std::integral Integer>
Integer integer(std::string_view text, std::size_t row, std::string_view name)
{
    Integer value{};
    const auto [end, error] = std::from_chars(text.data(), text.data() + text.size(), value);
    if (error != std::errc{} || end != text.data() + text.size())
        throw FixtureParseError("fixture row " + std::to_string(row) + ": invalid " +
                                std::string{name} + " '" + std::string{text} + "'");
    return value;
}

inline Date calendar_date(const std::string& text, std::size_t row, std::string_view name)
{
    if (text.empty())
        throw FixtureParseError("fixture row " + std::to_string(row) + ": empty " + std::string{name});
    if (text.size() == 10 && text[4] == '-' && text[7] == '-') {
        try {
            const std::string_view components{text};
            const Date value{std::chrono::year{integer<int>(components.substr(0, 4), row, name)} /
                             std::chrono::month{integer<unsigned>(components.substr(5, 2), row, name)} /
                             std::chrono::day{integer<unsigned>(components.substr(8, 2), row, name)}};
            if (is_supported_date(value)) return value;
        } catch (const FixtureParseError&) {
            // Report the complete date below rather than the individual component.
        }
    }
    throw FixtureParseError("fixture row " + std::to_string(row) + ": invalid " +
                            std::string{name} + " '" + text + "' (expected YYYY-MM-DD)");
}

inline void check_tolerance(double tolerance, std::size_t row, std::string_view name)
{
    if (tolerance < 0.0) {
        throw FixtureParseError("fixture row " + std::to_string(row) + ": " +
                                std::string{name} + " must be non-negative");
    }
}

inline FixtureAttributes attributes(std::string_view text, std::size_t row, std::string_view name)
{
    FixtureAttributes result;
    if (text.empty() || text == "-") return result;
    for (const auto& item : split(text, ';')) {
        const auto separator = item.find('=');
        if (separator == std::string::npos || separator == 0 || separator + 1 == item.size()) {
            throw FixtureParseError("fixture row " + std::to_string(row) + ": invalid " +
                                    std::string{name} + " entry '" + item + "'");
        }
        const auto key = item.substr(0, separator);
        if (result.contains(key)) {
            throw FixtureParseError("fixture row " + std::to_string(row) + ": duplicate " +
                                    std::string{name} + " key '" + key + "'");
        }
        result.emplace(key, item.substr(separator + 1));
    }
    return result;
}

inline std::map<std::string, double> numeric_attributes(std::string_view text, std::size_t row,
                                                        std::string_view name)
{
    std::map<std::string, double> result;
    for (const auto& [key, value] : attributes(text, row, name))
        result.emplace(key, number(value, row, std::string{name} + " value"));
    return result;
}

} // namespace detail

inline std::vector<ReferenceCase> parse_reference_cases(std::istream& input)
{
    using namespace detail;
    constexpr std::array columns{"case_id", "instrument", "engine", "variant", "inputs", "outputs",
                                 "tolerances"};
    std::string line;
    std::size_t row = 0;
    std::vector<ReferenceCase> cases;
    bool header_read = false;
    while (std::getline(input, line)) {
        ++row;
        if (!line.empty() && line.back() == '\r') line.pop_back();
        if (line.empty() || line.front() == '#') continue;
        if (!header_read) {
            const auto header = split(line, '\t');
            if (header.size() != columns.size())
                throw FixtureParseError("fixture header: expected " + std::to_string(columns.size()) +
                                        " columns, got " + std::to_string(header.size()));
            for (std::size_t index = 0; index < columns.size(); ++index)
                if (header[index] != columns[index])
                    throw FixtureParseError("fixture header: column " + std::to_string(index + 1) +
                                            " must be '" + columns[index] + "'");
            header_read = true;
            continue;
        }
        const auto fields = split(line, '\t');
        if (fields.size() != columns.size())
            throw FixtureParseError("fixture row " + std::to_string(row) + ": expected " +
                                    std::to_string(columns.size()) + " columns, got " +
                                    std::to_string(fields.size()));
        ReferenceCase value;
        value.case_id = field(fields, 0, row, "case_id");
        value.instrument = field(fields, 1, row, "instrument");
        value.engine = field(fields, 2, row, "engine");
        value.variant = field(fields, 3, row, "variant");
        value.inputs = attributes(fields[4], row, "inputs");
        const auto required_input = [&](std::string_view name) {
            const auto found = value.inputs.find(std::string{name});
            if (found == value.inputs.end() || found->second.empty())
                throw FixtureParseError("fixture row " + std::to_string(row) + ": missing provenance " + std::string{name});
            return found->second;
        };
        value.provenance = ReferenceProvenance{
            required_input("source_revision"), required_input("source_symbol"),
            required_input("convention"), required_input("reference_kind"), 0.0};
        if (value.provenance.reference_kind != "analytic" &&
            value.provenance.reference_kind != "approximate" &&
            value.provenance.reference_kind != "discretized" &&
            value.provenance.reference_kind != "statistical")
            throw FixtureParseError("fixture row " + std::to_string(row) +
                                    ": reference_kind must be analytic, approximate, discretized, or statistical");
        const auto tolerance_text = required_input("tolerance");
        try {
            value.provenance.explicit_tolerance = number(tolerance_text, row, "provenance tolerance");
            check_tolerance(value.provenance.explicit_tolerance, row, "provenance tolerance");
        } catch (const FixtureParseError&) {
            throw FixtureParseError("fixture row " + std::to_string(row) +
                                    ": provenance tolerance must be finite and non-negative");
        }
        value.outputs = numeric_attributes(fields[5], row, "outputs");
        value.tolerances = numeric_attributes(fields[6], row, "tolerances");
        if (value.outputs.size() != value.tolerances.size())
            throw FixtureParseError("fixture row " + std::to_string(row) +
                                    ": outputs and tolerances must have matching keys");
        for (const auto& output : value.outputs)
            if (!value.tolerances.contains(output.first))
                throw FixtureParseError("fixture row " + std::to_string(row) +
                                        ": outputs and tolerances must have matching keys");
        for (const auto& [name, tolerance] : value.tolerances)
            check_tolerance(tolerance, row, name + " tolerance");
        if (value.inputs.contains("owner") && value.inputs.at("owner") == "QuantLib") {
            const auto invalid = [&] {
                return FixtureParseError("fixture row " + std::to_string(row) + ": invalid Greek declaration");
            };
            const std::map<std::string, std::string> units{
                {"price", "price"}, {"delta", "price/spot"}, {"gamma", "price/spot^2"}, {"speed", "price/spot^3"}, {"theta", "price/day"}, {"charm", "delta/day"}, {"color", "gamma/day"}, {"vega", "price/volatility-pp"}, {"vanna", "delta/volatility-pp"}, {"zomma", "gamma/volatility-pp"}, {"rho", "price/rate-pp"}};
            const auto expiry_date = calendar_date(required_input("expiry_date"), row, "expiry_date");
            const auto valuation = calendar_date(required_input("valuation"), row, "valuation");
            const bool expiry_boundary = (expiry_date - valuation).count() <= 2;
            const bool exercise_boundary = value.instrument == "AmericanOption" &&
                                           (valuation - calendar_date(required_input("effective_date"), row, "effective_date")).count() < 2;
            const bool boundary = expiry_boundary || exercise_boundary;
            const bool binary_product = value.instrument == "BinaryBarrierOption" ||
                                        value.instrument == "TouchOption";
            const bool binary_expiry = binary_product && expiry_date == valuation;
            const bool asian = value.instrument == "GeometricAveragePriceOption" || value.instrument == "ArithmeticAveragePriceOption";
            const bool asian_expiry = asian && expiry_date == valuation;
            const bool asian_start = asian &&
                                     (valuation - calendar_date(required_input("averaging_start_date"), row, "averaging_start_date")).count() <= 2;
            const bool binary_boundary = binary_product &&
                                         number(required_input("spot"), row, "spot") ==
                                             number(required_input("barrier"), row, "barrier");
            std::size_t available = 0;
            for (const auto& [name, unit] : units) {
                const bool unavailable = ((binary_boundary || binary_expiry || asian_expiry || asian_start) && name != "price") ||
                                         (boundary && (name == "theta" || name == "charm" || name == "color"));
                if (required_input("unit_" + name) != unit ||
                    value.inputs.contains("unavailable_" + name) != unavailable ||
                    value.outputs.contains(name) == unavailable) throw invalid();
                if (unavailable) {
                    if (required_input("unavailable_" + name) != (asian_expiry ? "terminal average payoff: no smooth sensitivities" : asian_start   ? "averaging-start boundary: price only, no smooth time stencil"
                                                                                                                                  : binary_expiry   ? "terminal payoff: no smooth sensitivities"
                                                                                                                                  : binary_boundary ? "spot equals barrier: hit-state boundary"
                                                                                                                                  : expiry_boundary ? "whole-day stability stencil touches expiry_date"
                                                                                                                                                    : "whole-day stability stencil precedes exercise window"))
                        throw invalid();
                    continue;
                }
                ++available;
                const auto read = [&](const std::string& prefix) {
                    const double result = number(required_input(prefix + name), row, prefix + name);
                    check_tolerance(result, row, prefix + name);
                    return result;
                };
                if (read("uncertainty_") > read("stability_limit_")) throw invalid();
                (void)read("numerical_tolerance_");
            }
            if (value.outputs.size() != available) throw invalid();
            for (const auto& [name, reason] : value.inputs) {
                if (name.starts_with("unavailable_") && !units.contains(name.substr(12))) throw invalid();
            }
        }
        if (value.engine == "MonteCarloEuropeanEngine" || value.engine == "MonteCarloAmericanEngine") {
            (void)integer<std::uint64_t>(required_input("seed"), row, "seed");
            if (integer<int>(required_input("paths"), row, "paths") <= 0 ||
                integer<int>(required_input("steps"), row, "steps") <= 0)
                throw FixtureParseError("fixture row " + std::to_string(row) + ": paths and steps must be positive");
        }
        cases.push_back(std::move(value));
    }
    if (!header_read) throw FixtureParseError("fixture is missing a header");
    if (cases.empty()) throw FixtureParseError("fixture contains no data rows");
    return cases;
}

inline std::vector<ReferenceCase> load_reference_cases(const std::filesystem::path& path)
{
    std::ifstream input(path);
    if (!input) throw FixtureParseError("cannot open fixture '" + path.string() + "'");
    return parse_reference_cases(input);
}

} // namespace kiyosi::test
