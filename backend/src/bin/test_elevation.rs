use backend::elevation::fetch_elevations;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("Testing Open-Elevation API integration...\n");

    // Test with a few coordinates in the Rhône-Alpes region
    let test_coords = vec![
        (45.764, 4.835),   // Lyon
        (45.9305, 4.577),  // Villefranche-sur-Saône
        (45.999, 4.546),   // Near the node we found with elevation
    ];

    println!("Fetching elevations for {} coordinates:", test_coords.len());
    for (i, (lat, lon)) in test_coords.iter().enumerate() {
        println!("  {}: lat={}, lon={}", i + 1, lat, lon);
    }
    println!();

    match fetch_elevations(test_coords.clone()).await {
        Ok(elevations) => {
            println!("✅ Successfully fetched {} elevation values:\n", elevations.len());
            for (i, elev) in elevations.iter().enumerate() {
                let (lat, lon) = test_coords[i];
                println!("  Coordinate ({}, {}): {} meters", lat, lon, elev);
            }
            println!("\n✅ Elevation API integration is working!");
            Ok(())
        }
        Err(e) => {
            eprintln!("❌ Error fetching elevations: {}", e);
            Err(e)
        }
    }
}
