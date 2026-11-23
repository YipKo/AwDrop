using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Reflection;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace AwDrop.Windows;

internal sealed class Preferences
{
    private static readonly string Folder = Environment.GetEnvironmentVariable("AWDROP_DATA_DIR")
        ?? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "AwDrop");
    private static readonly string FilePath = Path.Combine(Folder, "preferences.json");

    [JsonPropertyName("language")] public string? Language { get; set; }
    [JsonPropertyName("appearance")] public string Appearance { get; set; } = "system";
    [JsonPropertyName("reduce_motion")] public bool ReduceMotion { get; set; }
    [JsonPropertyName("device_name")] public string? DeviceName { get; set; }
    [JsonPropertyName("receive_folder")] public string? ReceiveFolder { get; set; }
    [JsonPropertyName("ask_before_receiving")] public bool AskBeforeReceiving { get; set; } = true;

    public static Preferences Load()
    {
        try { return JsonSerializer.Deserialize<Preferences>(File.ReadAllText(FilePath)) ?? new(); }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException or JsonException) { return new(); }
    }

    public void Save()
    {
        Directory.CreateDirectory(Folder);
        string temporary = FilePath + ".tmp";
        File.WriteAllText(temporary, JsonSerializer.Serialize(this));
        File.Move(temporary, FilePath, overwrite: true);
    }
}

internal sealed class Strings
{
    public static readonly (string Code, string Name)[] Languages =
    [
        ("en", "English"), ("zh-CN", "简体中文"), ("zh-HK", "繁體中文（香港）"),
        ("zh-TW", "繁體中文（台灣）"), ("ja", "日本語"), ("fr", "Français"),
        ("ru", "Русский"), ("es", "Español"),
    ];

    private readonly Dictionary<string, string> values;
    public string Code { get; }

    public Strings(string? preference)
    {
        string tag = (preference ?? CultureInfo.CurrentUICulture.Name).ToLowerInvariant();
        Code = tag.Split('-')[0] switch
        {
            "zh" when tag.Contains("-hans") => "zh-CN",
            "zh" when tag.EndsWith("-hk") || tag.EndsWith("-mo") => "zh-HK",
            "zh" when tag.Contains("-hant") || tag.EndsWith("-tw") => "zh-TW",
            "zh" => "zh-CN",
            "ja" => "ja",
            "fr" => "fr",
            "ru" => "ru",
            "es" => "es",
            _ => "en",
        };

        using Stream? stream = Code == "en" ? null : Assembly.GetExecutingAssembly()
            .GetManifestResourceStream($"AwDrop.Locales.{Code}.json");
        values = stream == null ? new() : JsonSerializer.Deserialize<Dictionary<string, string>>(stream) ?? new();
    }

    public string this[string key] => values.TryGetValue(key, out string? value) ? value : key;
}
