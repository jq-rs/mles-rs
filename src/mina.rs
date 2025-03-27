use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;

pub async fn serve_status_page() -> Result<impl warp::Reply, warp::Rejection> {
    let html = generate_status_page().await;
    Ok(warp::reply::html(html))
}

async fn generate_status_page() -> String {
    let files = vec![
        (
            "Server 1",
            "/home/ubuntu/www/mles-rs/static/mles.io/mina/server1.txt",
        ),
        (
            "Server 2",
            "/home/ubuntu/www/mles-rs/static/mles.io/mina/server2.txt",
        ),
        (
            "Server 3",
            "/home/ubuntu/www/mles-rs/static/mles.io/mina/server3.txt",
        ),
        (
            "Server 4",
            "/home/ubuntu/www/mles-rs/static/mles.io/mina/server4.txt",
        ),
    ];

    let mut html = String::from(
        r#"
        <!DOCTYPE html>
        <html>
        <head>
            <title>qstake mina status</title>
            <meta http-equiv="refresh" content="5">
            <style>
                body {
                    font-family: monospace;
                }
                .status {
                    font-weight: bold;
                    margin: 10px 0;
                }
                .green { color: green; }
                .red { color: red; }
                .grey { color: grey; }
                .yellow { color: yellow; }
            </style>
        </head>
        <script>
        async function fetchMinaPrice() {
            const now = Date.now();
            const cache = getCache();

            // If cached data is valid (within 1 minute), use it
            if (cache && (now - cache.timestamp < 60 * 1000)) {
                updatePriceDisplay(cache.price);
                return;
            }

            // Otherwise, fetch new price
            try {
                const response = await fetch("https://api.coingecko.com/api/v3/simple/price?ids=mina-protocol&vs_currencies=eur");
                if (!response.ok) {
                    throw new Error(`HTTP error! Status: ${response.status}`);
                }
                const data = await response.json();
                const price = data['mina-protocol']?.eur;

                if (price) {
                    saveCache(price, now);
                    updatePriceDisplay(price);
                } else {
                    updatePriceDisplay(cache?.price); // Use optional chaining
                }
            } catch (error) {
                console.error("Error fetching Mina price:", error);
                updatePriceDisplay(cache?.price); // Use optional chaining
            }
        }

        function getCache() {
            try {
                const cache = JSON.parse(localStorage.getItem('minaPriceCache'));
                return cache?.price && cache?.timestamp ? cache : null;
            } catch {
                return null;
            }
        }

        function saveCache(price, timestamp) {
            const cache = { price, timestamp };
            localStorage.setItem('minaPriceCache', JSON.stringify(cache));
        }

        function updatePriceDisplay(price) {
            const priceElement = document.getElementById('mina-price');
            if (price) {
                priceElement.innerHTML = `Mina Price: <span class="green">€${price}</span>`;
            } else {
                priceElement.innerHTML = `Mina Price: <span class="red">Unavailable</span>`;
            }
        }

        // Call fetchMinaPrice immediately and set up periodic refresh
        document.addEventListener('DOMContentLoaded', () => {
            fetchMinaPrice();
            // Optionally refresh price every minute
            setInterval(fetchMinaPrice, 60000);
        });
        </script>
        <body>
            <h1>qstake mina status</h1>
        "#,
    );

    for (name, path) in files {
        let status = check_file_status(path).await;

        // Find the "last proof time" value directly from the iterator
        let mut last_proof_time: Option<u64> = None;
        let mut words = status.2.split_whitespace();
        let synced_status = words
            .next()
            .map(|word| word.trim_end_matches(','))
            .unwrap_or("Unknown");
        let mut color = "green";

        // Determine color for the synced status
        let synced_color = match synced_status {
            "Synced" => "green",
            "Catchup" => "yellow",
            _ => "unknown", // Red for anything else
        };

        while let Some(word) = words.next() {
            if word == "time" {
                if let Some(ms_value) = words.next() {
                    if let Ok(parsed_time) = ms_value.parse::<u64>() {
                        last_proof_time = Some(parsed_time);
                        break;
                    }
                }
            }
        }

        // Determine color based on the last proof time
        let proof_color = if let Some(parsed_time) = last_proof_time {
            if parsed_time < 100000 {
                "green"
            } else if parsed_time < 150000 {
                "yellow"
            } else {
                "red"
            }
        } else {
            "unknown"
        };

        if synced_color == "yellow" || proof_color == "yellow" {
            color = "yellow";
        }

        if synced_color == "red" || proof_color == "red" {
            color = "red";
        }

        html.push_str(&format!(
            r#"
            <div class="status">
                {}: <span class="{}">{} (<span class="{}">{}</span>)</span>
            </div>
            "#,
            name,
            if status.0 { "green" } else { "red" },
            status.1,
            color,
            status.2
        ));
    }

    html.push_str(&format!(
        r#"<div id="mina-price" class="status">Mina Price: <span class="grey">Loading...</span></div>"#
    ));

    html.push_str(
        r#"
        </body>
        </html>
        "#,
    );

    html
}

async fn check_file_status(path: &str) -> (bool, String, String) {
    match fs::metadata(path).await {
        Ok(metadata) => {
            if let Ok(modified) = metadata.modified() {
                if let Ok(duration) = modified.duration_since(UNIX_EPOCH) {
                    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
                    let elapsed_secs = now.as_secs() - duration.as_secs();

                    let content = match fs::read_to_string(path).await {
                        Ok(file_content) => file_content,
                        Err(_) => "Error reading file content".to_string(),
                    };

                    if elapsed_secs <= 60 {
                        return (true, format!("OK, {} seconds ago", elapsed_secs), content);
                    } else {
                        return (
                            false,
                            format!("Failed, {} seconds ago", elapsed_secs),
                            content,
                        );
                    }
                }
            }
            (
                false,
                "File exists but couldn't retrieve timestamp".to_string(),
                "".to_string(),
            )
        }
        Err(_) => (false, "File not found".to_string(), "".to_string()),
    }
}
