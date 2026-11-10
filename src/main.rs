
use chrono::{Datelike, Local, NaiveDate};
use reqwest::Client;
use serde_json::{json, Value};
use std::{env, error::Error};

const AUTH_URL: &str =
    "https://schule.louisenlund.de/api//Api/Authorization/JwtAuth";

const SCHEDULE_URL: &str =
    "https://schule.louisenlund.de/api//api/Schedule/GetScheduleCached";

#[derive(Debug, Clone)]
struct Lesson {
    teacher: String,
    subject: String,
    room: String,
    start: String,
    end: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();

    let username = env::var("A4S_USERNAME")?;
    let password = env::var("A4S_PASSWORD")?;

    let client = Client::builder().build()?;

    let today = Local::now().date_naive();
    let date = today.format("%Y-%m-%d").to_string();

    println!("Louisenlund Stundenplan");
    println!("Datum: {}", date);
    println!("--------------------------------");

    let token = get_token(&client, &username, &password).await?;

    println!("Login erfolgreich!");

    let schedule = get_schedule(&client, &token, today).await?;

    let lessons = extract_lessons(&schedule);

    let merged = merge_consecutive(lessons);

    print_schedule(&merged, &date);

    std::fs::create_dir_all("data")?;

    let output = serde_json::to_string_pretty(&schedule)?;

/*****

    std::fs::write(
        format!("data/schedule-{}.json", date),
        output,
    )?;

    println!("\nJSON-Datei wurde gespeichert.");

******/

println!("\nProgramm beendet.");
println!("Drücke Enter zum Schließen...");

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    Ok(())
}

async fn get_token(
    client: &Client,
    username: &str,
    password: &str,
) -> Result<String, Box<dyn Error>> {
    let payload = json!({
        "Username": username,
        "Password": password,
        "Pin": "null"
    });

    let response = client
        .post(AUTH_URL)
        .json(&payload)
        .send()
        .await?;

    let status = response.status();
    let body = response.text().await?;

    if !status.is_success() {
        return Err(format!("Login fehlgeschlagen: HTTP {}", status).into());
    }

    let token = body.trim().trim_matches('"').to_string();

    if token.is_empty() {
        return Err("Leerer Login-Token erhalten.".into());
    }

    Ok(token)
}

async fn get_schedule(
    client: &Client,
    token: &str,
    date: NaiveDate,
) -> Result<Value, Box<dyn Error>> {
    let date_string = date.format("%Y-%m-%d").to_string();

    let payload = [
        ("schoolId", "35".to_string()),
        ("from", format!("{}T01:00:00.000Z", date_string)),
        ("to", format!("{}T23:00:00.000Z", date_string)),
        ("getShortNames", "False".to_string()),
        ("getAbsences", "False".to_string()),
    ];

    let response = client
        .post(SCHEDULE_URL)
        .bearer_auth(token)
        .form(&payload)
        .send()
        .await?;

    let status = response.status();
    let body = response.text().await?;

    if !status.is_success() {
        return Err(
            format!("Stundenplan-Abfrage fehlgeschlagen: HTTP {}", status).into()
        );
    }

    let mut data: Value = serde_json::from_str(&body)?;

    // Manche Antworten enthalten JSON als String innerhalb von JSON.
    if let Value::String(inner) = &data {
        data = serde_json::from_str(inner)?;
    }

    Ok(data)
}

fn extract_lessons(schedule: &Value) -> Vec<Lesson> {
    let lessons = match schedule.get("lessons").and_then(Value::as_array) {
        Some(lessons) => lessons,
        None => {
            println!("Keine Lessons im API-Ergebnis gefunden.");
            return Vec::new();
        }
    };

    lessons
        .iter()
        .filter(|lesson| {
            is_truthy(lesson.get("SubjectRelevantAbsences"))
        })
        .map(|lesson| Lesson {
            teacher: get_text(lesson, "TeacherName"),
            subject: get_text(lesson, "SubjectName"),
            room: get_text(lesson, "RoomName"),
            start: get_text(lesson, "start"),
            end: get_text(lesson, "end"),
        })
        .collect()
}

fn is_truthy(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(value)) => value.as_i64().unwrap_or(0) != 0,
        Some(Value::String(value)) => {
            !value.is_empty() && value != "false" && value != "0"
        }
        _ => false,
    }
}

fn get_text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("-")
        .to_string()
}

fn merge_consecutive(mut lessons: Vec<Lesson>) -> Vec<Lesson> {
    lessons.sort_by(|a, b| a.start.cmp(&b.start));

    let mut merged: Vec<Lesson> = Vec::new();

    for lesson in lessons {
        if let Some(last) = merged.last_mut() {
            let same_class =
                last.teacher == lesson.teacher
                && last.subject == lesson.subject
                && last.room == lesson.room;

            if same_class && last.end == lesson.start {
                last.end = lesson.end;
                continue;
            }
        }

        merged.push(lesson);
    }

    merged
}

fn print_schedule(lessons: &[Lesson], date: &str) {
    let weekday = Local::now().weekday();

    let weekday_name = match weekday {
        chrono::Weekday::Mon => "Montag",
        chrono::Weekday::Tue => "Dienstag",
        chrono::Weekday::Wed => "Mittwoch",
        chrono::Weekday::Thu => "Donnerstag",
        chrono::Weekday::Fri => "Freitag",
        chrono::Weekday::Sat => "Samstag",
        chrono::Weekday::Sun => "Sonntag",
    };

    println!("\n{} ({})", weekday_name, date);
    println!("--------------------------------");

    let mut count = 0;

    for lesson in lessons {
        if !lesson.start.starts_with(date) {
            continue;
        }

        count += 1;

        let start = time_part(&lesson.start);
        let end = time_part(&lesson.end);

        println!(
            "{} - {} | {} | {} | {}",
            start,
            end,
            lesson.subject,
            lesson.teacher,
            lesson.room
        );
    }

    if count == 0 {
        println!("Keine passenden Unterrichtsstunden gefunden.");
    }
}

fn time_part(value: &str) -> String {
    if value.len() >= 16 {
        value[11..16].to_string()
    } else {
        value.to_string()
    }
}
