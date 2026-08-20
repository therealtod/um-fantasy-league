package com.umfl

import org.springframework.boot.autoconfigure.SpringBootApplication
import org.springframework.boot.context.properties.ConfigurationPropertiesScan
import org.springframework.boot.runApplication

@SpringBootApplication
@ConfigurationPropertiesScan
class UmFantasyLeagueApplication

fun main(args: Array<String>) {
    runApplication<UmFantasyLeagueApplication>(*args)
}
