using System.ComponentModel;
using System.Globalization;
using System.Reflection;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace Winawdl.Localization;

public sealed record LanguageOption(string Code, string Name);

/// <summary>Translation catalog. English source strings are stable keys and
/// missing entries fall back to English. Values may use numbered format fields.</summary>
public sealed class Strings : INotifyPropertyChanged
{
    public static Strings Current { get; } = new();
    public static IReadOnlyList<LanguageOption> Languages { get; } =
    [
        new("en", "English"), new("zh-CN", "简体中文"), new("zh-HK", "繁體中文（香港）"),
        new("zh-TW", "繁體中文（台灣）"), new("ja", "日本語"), new("fr", "Français"), new("ru", "Русский"),
        new("es", "Español"),
    ];

    private Dictionary<string, string> _catalog = new(StringComparer.Ordinal);
    private readonly Dictionary<string, string> _source = LoadSource();
    public string Code { get; private set; } = "en";
    public event PropertyChangedEventHandler? PropertyChanged;
    public string this[string key] => T(_source.TryGetValue(key, out var value) ? value : key);

    private static Dictionary<string, string> LoadSource()
    {
        using var stream = Assembly.GetExecutingAssembly().GetManifestResourceStream("winawdl.locales.en.json");
        return stream is null ? new(StringComparer.Ordinal)
            : JsonSerializer.Deserialize<Dictionary<string, string>>(stream) ?? new(StringComparer.Ordinal);
    }

    public static string Resolve(string? preference)
    {
        var value = preference ?? CultureInfo.CurrentUICulture.Name;
        if (Languages.Any(language => language.Code.Equals(value, StringComparison.OrdinalIgnoreCase)))
            return Languages.First(language => language.Code.Equals(value, StringComparison.OrdinalIgnoreCase)).Code;
        if (value.StartsWith("zh-HK", StringComparison.OrdinalIgnoreCase) || value.StartsWith("zh-MO", StringComparison.OrdinalIgnoreCase)) return "zh-HK";
        if (value.StartsWith("zh-TW", StringComparison.OrdinalIgnoreCase) || value.StartsWith("zh-Hant", StringComparison.OrdinalIgnoreCase)) return "zh-TW";
        if (value.StartsWith("zh", StringComparison.OrdinalIgnoreCase)) return "zh-CN";
        foreach (var code in new[] { "ja", "fr", "ru", "es" })
            if (value.StartsWith(code, StringComparison.OrdinalIgnoreCase)) return code;
        return "en";
    }

    public void Set(string? preference)
    {
        var code = Resolve(preference);
        Dictionary<string, string> catalog = new(StringComparer.Ordinal);
        if (code != "en")
        {
            using var stream = Assembly.GetExecutingAssembly().GetManifestResourceStream($"winawdl.locales.{code}.json");
            if (stream is not null)
                catalog = JsonSerializer.Deserialize<Dictionary<string, string>>(stream) ?? catalog;
        }
        _catalog = catalog;
        Code = code;
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs("Item[]"));
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(Code)));
    }

    public static string T(string source) => Current._catalog.TryGetValue(source, out var value) ? value : source;
    public static string F(string source, params object?[] args) => string.Format(CultureInfo.CurrentCulture, T(source), args);

    /// <summary>Formatted progress emitted by the model. Unknown diagnostics
    /// stay untouched; filenames and device names are never translated.</summary>
    public static string Status(string source)
    {
        var progress = Regex.Match(source, @"^(\d+%) of (.+?)( · .+)?$");
        if (progress.Success)
            return F("{0} of {1}", progress.Groups[1].Value, progress.Groups[2].Value) + progress.Groups[3].Value;
        if (source.EndsWith(" so far", StringComparison.Ordinal)) return F("{0} so far", source[..^7]);
        return T(source);
    }
}
