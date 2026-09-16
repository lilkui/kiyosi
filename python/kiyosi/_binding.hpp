#pragma once

#include <nanobind/nanobind.h>
#include <nanobind/stl/chrono.h>
#include <nanobind/stl/optional.h>
#include <nanobind/stl/string.h>
#include <nanobind/stl/vector.h>

#include <chrono>
#include <cstdint>
#include <exception>
#include <initializer_list>
#include <optional>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include <kiyosi/kiyosi.hpp>

namespace nb = nanobind;

namespace kiyosi::python_binding {

struct PythonDateAnnotation {};
struct PythonTimestampAnnotation {};
struct PythonValuationTimeAnnotation {};

} // namespace kiyosi::python_binding

namespace nanobind::detail {

template <>
struct type_caster<kiyosi::python_binding::PythonDateAnnotation> {
    NB_TYPE_CASTER(kiyosi::python_binding::PythonDateAnnotation,
                   const_name("datetime.date"))
};

template <>
struct type_caster<kiyosi::python_binding::PythonTimestampAnnotation> {
    NB_TYPE_CASTER(kiyosi::python_binding::PythonTimestampAnnotation,
                   const_name("datetime.datetime"))
};

template <>
struct type_caster<kiyosi::python_binding::PythonValuationTimeAnnotation> {
    NB_TYPE_CASTER(kiyosi::python_binding::PythonValuationTimeAnnotation,
                   const_name("datetime.date | datetime.datetime"))
};

} // namespace nanobind::detail

namespace kiyosi::python_binding {

using PythonReal = nb::typed<nb::handle, double>;
using PythonInteger = nb::typed<nb::handle, int>;
using PythonDate = nb::typed<nb::handle, PythonDateAnnotation>;
using PythonValuationTime = nb::typed<nb::handle, PythonValuationTimeAnnotation>;
using PythonRealSequence = nb::typed<nb::handle, nb::typed<nb::iterable, double>>;
using PythonDateSequence =
    nb::typed<nb::handle, nb::typed<nb::iterable, PythonDateAnnotation>>;
using PythonOptionType = nb::typed<nb::object, option_type>;
using PythonDateObject = nb::typed<nb::object, PythonDateAnnotation>;
using PythonTimestampObject = nb::typed<nb::object, PythonTimestampAnnotation>;
using PythonDateList = nb::typed<nb::list, PythonDateAnnotation>;
using PythonDateIterator =
    nb::typed<nb::object, nb::typed<nb::iterator, PythonDateAnnotation>>;
using PythonStringIterator = nb::typed<nb::object, nb::typed<nb::iterator, std::string>>;
using PythonOptionalReal = nb::typed<nb::object, std::optional<double>>;

struct ReprField {
    const char* name;
    const char* attribute;
};

template <typename T>
void bind_repr(nb::class_<T>& binding, const char* name,
               std::vector<ReprField> fields)
{
    const std::string type_name{name};
    const std::vector<ReprField> attributes{std::move(fields)};
    binding.def("__repr__", [type_name, attributes](const T& value) {
        const nb::object self = nb::cast(&value, nb::rv_policy::reference);
        nb::list parts;
        for (const auto& [field, attribute] : attributes) {
            parts.append(nb::str("{}={!r}").attr("format")(
                nb::str(field), self.attr(attribute)));
        }
        return nb::str("{}({})").attr("format")(
            nb::str(type_name.c_str()), nb::str(", ").attr("join")(parts));
    });
}

template <typename T>
void bind_value_equality(nb::class_<T>& binding)
{
    binding.def("__eq__", [](const T& left, const T& right) { return left == right; },
                nb::is_operator());
    binding.attr("__hash__") = nb::none();
}

class DomainException final : public std::runtime_error {
public:
    explicit DomainException(Error error)
        : std::runtime_error(std::move(error.message)), category_(error.category) {}

    error_category category() const noexcept { return category_; }

private:
    error_category category_;
};

template <typename T>
T unwrap(result<T> value)
{
    if (!value) throw DomainException{std::move(value.error())};
    return std::move(*value);
}

inline void unwrap(result<void> value)
{
    if (!value) throw DomainException{std::move(value.error())};
}

[[noreturn]] inline void type_error(std::string_view field, std::string_view expected)
{
    throw nb::type_error((std::string{field} + " must be " + std::string{expected}).c_str());
}

inline double real_number(nb::handle value, std::string_view field)
{
    const nb::object real_type = nb::module_::import_("numbers").attr("Real");
    const int is_real = PyObject_IsInstance(value.ptr(), real_type.ptr());
    if (is_real < 0) throw nb::python_error();
    if (PyBool_Check(value.ptr()) || is_real == 0) type_error(field, "a real number");
    const double converted = PyFloat_AsDouble(value.ptr());
    if (PyErr_Occurred()) throw nb::python_error();
    return converted;
}

inline int integer(nb::handle value, std::string_view field)
{
    const nb::object integer_type = nb::module_::import_("numbers").attr("Integral");
    const int is_integer = PyObject_IsInstance(value.ptr(), integer_type.ptr());
    if (is_integer < 0) throw nb::python_error();
    if (PyBool_Check(value.ptr()) || is_integer == 0) type_error(field, "an integer");
    const long long converted = PyLong_AsLongLong(value.ptr());
    if (PyErr_Occurred()) throw nb::python_error();
    if (!std::in_range<int>(converted))
        throw std::overflow_error(std::string{field} + " is outside the range of a C++ int");
    return static_cast<int>(converted);
}

inline std::optional<std::uint64_t> optional_seed(nb::handle value)
{
    if (value.is_none()) return std::nullopt;
    const nb::object integer_type = nb::module_::import_("numbers").attr("Integral");
    const int is_integer = PyObject_IsInstance(value.ptr(), integer_type.ptr());
    if (is_integer < 0) throw nb::python_error();
    if (PyBool_Check(value.ptr()) || is_integer == 0)
        type_error("seed", "a non-negative integer or None");
    const unsigned long long converted = PyLong_AsUnsignedLongLong(value.ptr());
    if (PyErr_Occurred()) throw nb::python_error();
    if (!std::in_range<std::uint64_t>(converted))
        throw std::overflow_error("seed is outside the range of a uint64_t");
    return static_cast<std::uint64_t>(converted);
}

inline date calendar_date(nb::handle value, std::string_view field)
{
    const nb::object datetime_module = nb::module_::import_("datetime");
    const nb::object date_type = datetime_module.attr("date");
    const nb::object datetime_type = datetime_module.attr("datetime");
    const int is_date = PyObject_IsInstance(value.ptr(), date_type.ptr());
    const int is_datetime = PyObject_IsInstance(value.ptr(), datetime_type.ptr());
    if (is_date < 0 || is_datetime < 0) throw nb::python_error();
    if (is_date == 0 || is_datetime != 0) type_error(field, "a datetime.date");
    const nb::object object = nb::borrow<nb::object>(value);
    return date{std::chrono::year{nb::cast<int>(object.attr("year"))} /
                std::chrono::month{nb::cast<unsigned>(object.attr("month"))} /
                std::chrono::day{nb::cast<unsigned>(object.attr("day"))}};
}

inline PythonDateObject python_date(date value)
{
    const auto parts = std::chrono::year_month_day{value};
    const nb::object datetime = nb::module_::import_("datetime");
    return PythonDateObject{datetime.attr("date")(
        int(parts.year()), static_cast<unsigned>(parts.month()),
        static_cast<unsigned>(parts.day()))};
}

inline PythonTimestampObject python_timestamp(timestamp value)
{
    const date day = std::chrono::floor<std::chrono::days>(value);
    const std::chrono::hh_mm_ss time{
        std::chrono::floor<std::chrono::microseconds>(value - day)};
    const auto parts = std::chrono::year_month_day{day};
    const nb::object datetime = nb::module_::import_("datetime");
    return PythonTimestampObject{datetime.attr("datetime")(
        int(parts.year()), static_cast<unsigned>(parts.month()),
        static_cast<unsigned>(parts.day()), time.hours().count(), time.minutes().count(),
        time.seconds().count(), time.subseconds().count(),
        datetime.attr("timezone").attr("utc"))};
}

inline timestamp valuation_time(nb::handle value)
{
    const nb::object datetime_module = nb::module_::import_("datetime");
    const nb::object datetime_type = datetime_module.attr("datetime");
    const int is_datetime = PyObject_IsInstance(value.ptr(), datetime_type.ptr());
    if (is_datetime < 0) throw nb::python_error();
    if (is_datetime == 0) return start_of_day(calendar_date(value, "valuation_time"));

    const nb::object object = nb::borrow<nb::object>(value);
    if (object.attr("utcoffset")().is_none())
        type_error("valuation_time", "a date or timezone-aware datetime");
    const nb::object utc = object.attr("astimezone")(datetime_module.attr("timezone").attr("utc"));
    const date day{std::chrono::year{nb::cast<int>(utc.attr("year"))} /
                   std::chrono::month{nb::cast<unsigned>(utc.attr("month"))} /
                   std::chrono::day{nb::cast<unsigned>(utc.attr("day"))}};
    return start_of_day(day) + std::chrono::hours{nb::cast<int>(utc.attr("hour"))} +
           std::chrono::minutes{nb::cast<int>(utc.attr("minute"))} +
           std::chrono::seconds{nb::cast<int>(utc.attr("second"))} +
           std::chrono::microseconds{nb::cast<int>(utc.attr("microsecond"))};
}

inline std::vector<date> date_sequence(nb::handle values, std::string_view field)
{
    const nb::object iterator = nb::steal<nb::object>(PyObject_GetIter(values.ptr()));
    if (!iterator.is_valid()) type_error(field, "an iterable of datetime.date values");
    std::vector<date> output;
    while (PyObject* item = PyIter_Next(iterator.ptr())) {
        const nb::object owned = nb::steal<nb::object>(item);
        output.push_back(calendar_date(owned, field));
    }
    if (PyErr_Occurred()) throw nb::python_error();
    return output;
}

inline std::vector<double> real_sequence(nb::handle values, std::string_view field)
{
    const nb::object iterator = nb::steal<nb::object>(PyObject_GetIter(values.ptr()));
    if (!iterator.is_valid()) type_error(field, "an iterable of real numbers");
    std::vector<double> output;
    while (PyObject* item = PyIter_Next(iterator.ptr())) {
        const nb::object owned = nb::steal<nb::object>(item);
        output.push_back(real_number(owned, field));
    }
    if (PyErr_Occurred()) throw nb::python_error();
    return output;
}

void bind_enums(nb::module_& module);
void bind_market(nb::module_& module);
void bind_instruments(nb::module_& module);
void bind_structured_instruments(nb::module_& module);
void bind_results(nb::module_& module);
void bind_engines(nb::module_& module);
void bind_analytics(nb::module_& module);

} // namespace kiyosi::python_binding
